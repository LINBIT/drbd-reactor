use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::io::{Error, ErrorKind};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{crate_authors, crate_version, App, AppSettings, Arg, ArgMatches, Shell, SubCommand};
use colored::Colorize;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use signal_hook::iterator::Signals;
use tempfile::NamedTempFile;
use tinytemplate::TinyTemplate;

use drbd_reactor::config;
use drbd_reactor::drbd;
use drbd_reactor::plugin;
use drbd_reactor::plugin::promoter;

static TERMINATE: AtomicBool = AtomicBool::new(false);

const REACTOR_RELOAD_PATH: &str = "drbd-reactor-reload.path";
const REACTOR_SERVICE: &str = "drbd-reactor.service";

fn main() -> Result<()> {
    let mut signals = Signals::new(&[libc::SIGINT, libc::SIGTERM])?;
    thread::spawn(move || {
        for _ in signals.forever() {
            TERMINATE.store(true, Ordering::Relaxed);
        }
    });

    let matches = get_app().get_matches();

    if let Some(compl_matches) = matches.subcommand_matches("generate-completion") {
        let shell = Shell::from_str(
            compl_matches
                .value_of("shell")
                .expect("expected to have a default"),
        )
        .expect("expected shell to be parsable"); // this has to be one of its variants.
        get_app().gen_completions_to("drbd-reactorctl", shell, &mut io::stdout());
        return Ok(());
    }

    let config_file = matches
        .value_of("config")
        .expect("expected to have a default");
    let snippets_path = get_snippets_path(&PathBuf::from(config_file))
        .with_context(|| "Could not get snippets path from config file")?;
    let snippets_path = PathBuf::from(snippets_path);

    match matches.subcommand() {
        ("cat", Some(cat_matches)) => cat(expand_snippets(&snippets_path, cat_matches, false)),
        ("disable", Some(disable_matches)) => {
            let now = disable_matches.is_present("now");
            disable(expand_snippets(&snippets_path, disable_matches, false), now)
        }
        ("enable", Some(enable_matches)) => {
            enable(expand_snippets(&snippets_path, enable_matches, true))
        }
        ("edit", Some(edit_matches)) => {
            let disabled = edit_matches.is_present("disabled");
            let force = edit_matches.is_present("force");
            let type_opt = edit_matches
                .value_of("type")
                .expect("expected to have a default");
            edit(
                expand_snippets(&snippets_path, edit_matches, disabled),
                &snippets_path,
                type_opt,
                force,
            )
        }
        ("evict", Some(evict_matches)) => {
            let force = evict_matches.is_present("force");
            let keep_masked = evict_matches.is_present("keep_masked");
            let unmask = evict_matches.is_present("unmask");
            let delay = evict_matches
                .value_of("delay")
                .expect("expected to have a default");
            let delay = delay.parse().expect("expected to be checked by parser");
            evict(
                expand_snippets(&snippets_path, evict_matches, false),
                force,
                keep_masked,
                unmask,
                delay,
            )
        }
        ("ls", Some(ls_matches)) => {
            let disabled = ls_matches.is_present("disabled");
            ls(expand_snippets(&snippets_path, ls_matches, disabled))
        }
        ("restart", Some(restart_matches)) => {
            let with_targets = restart_matches.is_present("with_targets");
            let configs = match restart_matches.values_of("configs") {
                None => Vec::new(),
                Some(_) => expand_snippets(&snippets_path, restart_matches, false),
            };
            restart(configs, with_targets)
        }
        ("rm", Some(rm_matches)) => {
            let force = rm_matches.is_present("force");
            let disabled = rm_matches.is_present("disabled");
            rm(expand_snippets(&snippets_path, rm_matches, disabled), force)
        }
        ("status", Some(status_matches)) => {
            let verbose = status_matches.is_present("verbose");
            let resources = status_matches.values_of("resource").unwrap_or_default();
            let resources: Vec<String> = resources.map(String::from).collect::<Vec<_>>();
            status(
                expand_snippets(&snippets_path, status_matches, false),
                verbose,
                &resources,
            )
        }
        _ => {
            // pretend it is status
            let args: ArgMatches = Default::default();
            status(
                expand_snippets(&snippets_path, &args, false),
                false,
                &vec![],
            )
        }
    }
}

fn ask(question: &str, default: bool) -> Result<bool> {
    print!("{} ", question);
    if default {
        print!("[Y/n] ");
    } else {
        print!("[N/y] ");
    }
    if io::stdout().flush().is_err() {
        return Err(anyhow::anyhow!("Could not flush stdout"));
    }

    loop {
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            eprintln!("Could not read answer from stdin");
            return Ok(false);
        }

        match input.trim().to_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            x => println!("Unknown answer '{}', use 'y' or 'n'", x),
        }
    }
}

fn edit(
    snippets_paths: Vec<PathBuf>,
    snippets_path: &PathBuf,
    type_opt: &str,
    force: bool,
) -> Result<()> {
    let editor = env::var("EDITOR").unwrap_or("vi".to_string());

    let len_err =
        || -> Result<()> { Err(anyhow::anyhow!("Expected excactly one {} plugin", type_opt)) };

    let mut persisted = 0;
    for snippet in &snippets_paths {
        // use new_in() to avoid $TMPDIR being on a different mount point than snippets_path
        // as this would result in an error on .persist()
        let mut tmpfile = NamedTempFile::new_in(&snippets_path)?;
        if snippet.exists() {
            fs::copy(snippet, tmpfile.path())?;
        } else {
            let id = basename_without_ext(snippet)?;
            let id = id
                .to_str()
                .ok_or(anyhow::anyhow!("Could not convert basaname to str"))?;
            let template = match type_opt {
                "promoter" => generate_template(PROMOTER_TEMPLATE, &id)?,
                "prometheus" => generate_template(PROMETHEUS_TEMPLATE, &id)?,
                "umh" => generate_template(UMH_TEMPLATE, &id)?,
                "debugger" => generate_template(DEBUGGER_TEMPLATE, &id)?,
                x => return Err(anyhow::anyhow!("Unknown type ('{}') to edit", x)),
            };
            tmpfile.write_all(template.as_bytes())?;
            tmpfile.flush()?;
        }
        plugin::map_status(Command::new(&editor).arg(tmpfile.path()).status())?;

        let content = fs::read_to_string(tmpfile.path())?;
        let config: config::Config = toml::from_str(&content)?;
        let plugins = config.plugins;
        if nr_plugins(&plugins) != 1 {
            // don't even want that to be force-able
            return Err(anyhow::anyhow!("Expected exactly 1 plugin configuration"));
        }

        if type_opt == "promoter" {
            if plugins.promoter.len() != 1 {
                return len_err();
            }
            for promoter in plugins.promoter {
                for config in promoter.resources.values() {
                    if let Some(last) = config.start.last() {
                        if last.ends_with(".mount") {
                            let err = "Mount unit should not be the topmost unit, consider using an OCF file system RA";
                            if force {
                                warn(err);
                            } else {
                                return Err(anyhow::anyhow!(err));
                            }
                        }
                    }
                }
            }
        } else if type_opt == "prometheus" {
            if plugins.prometheus.len() != 1 {
                return len_err();
            }
        } else if type_opt == "umh" {
            if plugins.umh.len() != 1 {
                return len_err();
            }
        } else if type_opt == "debugger" {
            if plugins.debugger.len() != 1 {
                return len_err();
            }
        } else {
            return Err(anyhow::anyhow!("Unknown type ('{}') to edit", type_opt));
        }

        tmpfile.persist(snippet)?;
        persisted += 1;
    }

    if persisted > 0 && !has_autoload()? {
        reload_service()?;
    }

    Ok(())
}

fn rm(snippets_paths: Vec<PathBuf>, force: bool) -> Result<()> {
    let mut removed = 0;
    for snippet in &snippets_paths {
        if !snippet.exists() {
            warn(&format!(
                "'{}' does not exist, doing nothing",
                snippet.display()
            ));
            continue;
        }
        eprintln!("Removing '{}'...", snippet.display());
        if force || ask(&format!("Remove '{}'?", snippet.display()), false)? {
            fs::remove_file(snippet)?;
            removed += 1;
        }
    }
    if removed > 0 && !has_autoload()? {
        reload_service()?;
    }
    Ok(())
}

fn enable(snippets_paths: Vec<PathBuf>) -> Result<()> {
    let mut enabled = 0;
    for snippet in &snippets_paths {
        if !snippet.exists() {
            warn(&format!(
                "'{}' does not exist, doing nothing",
                snippet.display()
            ));
            continue;
        }
        eprintln!("Enabling '{}'...", snippet.display());
        let enabled_path = get_enabled_path(&snippet)?;
        if enabled_path.exists() {
            warn(&format!(
                "'{}' already exists, doing nothing",
                enabled_path.display()
            ));
            continue;
        }
        fs::rename(snippet, enabled_path)?;
        enabled += 1;
    }

    if enabled > 0 && !has_autoload()? {
        reload_service()?;
    }

    Ok(())
}

fn stop_targets(snippets_paths: Vec<PathBuf>) -> Result<()> {
    for snippet in &snippets_paths {
        let conf = read_config(&snippet)?;
        for promoter in conf.plugins.promoter {
            for drbd_res in promoter.resources.keys() {
                let target = promoter::escaped_services_target(&drbd_res);
                systemctl(vec!["stop".into(), target])?;
            }
        }
    }

    Ok(())
}

fn disable(snippets_paths: Vec<PathBuf>, with_targets: bool) -> Result<()> {
    let mut disabled_snippets_paths: Vec<PathBuf> = Vec::new();
    for snippet in &snippets_paths {
        if !snippet.exists() {
            warn(&format!(
                "'{}' does not exist, doing nothing",
                snippet.display()
            ));
            continue;
        }
        eprintln!("Disabling '{}'...", snippet.display());
        let disabled_path = get_disabled_path(&snippet);
        fs::rename(snippet, disabled_path.clone())?;
        disabled_snippets_paths.push(disabled_path);
    }
    // we have to keep this order
    // reload first, so that a stop does not trigger a start again
    if !disabled_snippets_paths.is_empty() && !has_autoload()? {
        reload_service()?;
    }
    if with_targets {
        stop_targets(disabled_snippets_paths)?;
    }

    Ok(())
}

fn get_disabled_path(snippet_path: &PathBuf) -> PathBuf {
    snippet_path.with_extension("toml.disabled")
}

fn get_enabled_path(snippet_path: &PathBuf) -> Result<PathBuf> {
    match snippet_path.extension().and_then(|p| p.to_str()) {
        Some("disabled") => Ok(snippet_path.with_extension("")),
        Some(_) => Err(anyhow::anyhow!(
            "Expected plugin path '{}' to end in .disabled",
            snippet_path.display()
        )),
        None => Err(anyhow::anyhow!(
            "Expected to get proper extension for plugin path '{}'",
            snippet_path.display()
        )),
    }
}

fn basename_without_ext(snippet_path: &PathBuf) -> Result<PathBuf> {
    // in this case it is a bit more comfortable to handle that as string
    let file_name = snippet_path
        .file_name()
        .ok_or(anyhow::anyhow!(
            "Could not get file name from '{:?}",
            snippet_path.display()
        ))?
        .to_str()
        .ok_or(anyhow::anyhow!(
            "Could not convert file name '{:?}' to str",
            snippet_path.display()
        ))?;

    if file_name.ends_with(".toml") {
        Ok(PathBuf::from(file_name).with_extension(""))
    } else if file_name.ends_with(".toml.disabled") {
        Ok(PathBuf::from(file_name)
            .with_extension("")
            .with_extension(""))
    } else {
        Err(anyhow::anyhow!(
            "Not one of the expected extensions ('.toml' or '.toml.disabled')"
        ))
    }
}

fn has_autoload() -> Result<bool> {
    let status = Command::new("systemctl")
        .arg("is-active")
        .arg("-q")
        .arg(REACTOR_RELOAD_PATH)
        .status()?;
    Ok(status.success())
}

fn reload_service() -> Result<()> {
    systemctl(vec!["reload".into(), REACTOR_SERVICE.into()])
}

fn status(snippets_paths: Vec<PathBuf>, verbose: bool, resources: &Vec<String>) -> Result<()> {
    for snippet in snippets_paths {
        println!("{}:", snippet.display());
        let conf = read_config(&snippet)?;
        let plugins = conf.plugins;
        let me = promoter::uname_n()?;
        for promoter in plugins.promoter {
            print_promoter_id(&promoter);
            for (drbd_res, config) in promoter.resources {
                // check if in filter
                if !resources.is_empty() && !resources.contains(&drbd_res) {
                    continue;
                }
                let target = promoter::escaped_services_target(&drbd_res);
                let primary = get_primary(&drbd_res).unwrap_or(UNKNOWN.to_string());
                let primary = if primary == me {
                    "this node".to_string()
                } else {
                    format!("node '{}'", primary)
                };
                println!("Currently active on {}", primary);
                // target itself and the implicit one
                let promote_service = format!(
                    "drbd-promote@{}.service",
                    plugin::promoter::escape_name(&drbd_res)
                );
                if verbose {
                    systemctl(vec!["status".into(), "--no-pager".into(), target])?;
                    systemctl(vec!["status".into(), "--no-pager".into(), promote_service])?;
                } else {
                    println!("{} {}", status_dot(&target)?, target);
                    println!("{} ├─ {}", status_dot(&promote_service)?, promote_service);
                }
                // the implicit one
                let ocf_pattern = Regex::new(plugin::promoter::OCF_PATTERN)?;
                for (i, start) in config.start.iter().enumerate() {
                    let start = start.trim();
                    let (service_name, _) = match ocf_pattern.captures(start) {
                        Some(ocf) => {
                            let (vendor, agent, args) = (&ocf[1], &ocf[2], &ocf[3]);
                            plugin::promoter::escaped_systemd_ocf_parse_to_env(
                                &drbd_res, vendor, agent, args,
                            )?
                        }
                        _ => (start.to_string(), Vec::new()),
                    };
                    if verbose {
                        systemctl(vec!["status".into(), "--no-pager".into(), service_name])?;
                    } else {
                        let sep = if i == config.start.len() - 1 {
                            "└─"
                        } else {
                            "├─"
                        };
                        println!(
                            "{} {} {} {}",
                            status_dot(&service_name)?,
                            sep,
                            service_name,
                            freezer_state(&service_name)?
                        );
                    }
                }
            }
        }
        for prometheus in plugins.prometheus {
            print_prometheus_id(&prometheus);
            green(&format!("listening on {}", prometheus.address));
            if verbose {
                let addr: SocketAddr = prometheus.address.parse()?;
                let status = match prometheus_connect(&addr) {
                    Ok(_) => format!("{}", "success".bold().green()),
                    Err(e) => {
                        format!("{} ({})", "failed".bold().red(), e)
                    }
                };
                println!("TCP Connect: {}", status);
            }
        }
        for debugger in plugins.debugger {
            print_debugger_id(&debugger);
            println!("{}", "started".bold().green());
        }
        for umh in plugins.umh {
            print_umh_id(&umh);
            println!("{}", "started".bold().green());
        }
    }
    Ok(())
}

fn cat(snippets_paths: Vec<PathBuf>) -> Result<()> {
    for snippet in snippets_paths {
        if !snippet.exists() {
            warn(&format!(
                "'{}' does not exist, doing nothing",
                snippet.display()
            ));
            continue;
        }
        eprintln!("Displaying {}...", snippet.display());
        for catter in vec!["bat", "batcat", "cat"] {
            if plugin::map_status(Command::new(catter).arg(&snippet).status()).is_ok() {
                break;
            }
        }
    }
    Ok(())
}

fn evict_unmask_and_start(drbd_resources: &Vec<String>) -> Result<()> {
    for drbd_res in drbd_resources {
        let target = promoter::escaped_services_target(drbd_res);
        println!("Re-enabling {}", drbd_res);

        // old (at least RHEL8) systemctl allows you to mask --runtime, but does not allow unmask --runtime
        // we know that we created the thing via mask
        let path = "/run/systemd/system/".to_owned() + &target;
        fs::remove_file(Path::new(&path))?;
        println!("Removed {}.", path); // like systemctl unmaks would print it
        systemctl(vec!["daemon-reload".into()])?;

        // fails intentional if Primary on other node
        let _ = Command::new("systemctl")
            .arg("start")
            .arg(target)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    Ok(())
}

const UNKNOWN: &str = "<unknown>";
fn get_primary(drbd_resource: &str) -> Result<String> {
    let output = Command::new("drbdsetup")
        .arg("status")
        .arg("--json")
        .arg(drbd_resource)
        .output()?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "'drbdsetup show' not executed successfully"
        ));
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "kebab-case")]
    struct Resource {
        role: drbd::Role,
        connections: Vec<Connection>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "kebab-case")]
    struct Connection {
        name: String,
        peer_role: drbd::Role,
    }
    let resources: Vec<Resource> = serde_json::from_slice(&output.stdout)?;
    if resources.len() != 1 {
        return Err(anyhow::anyhow!(
            "resources lenght from drbdsetup status not exactly 1"
        ));
    }

    // is it me?
    if resources[0].role == drbd::Role::Primary {
        return promoter::uname_n();
    }

    // a peer?
    for conn in &resources[0].connections {
        if conn.peer_role == drbd::Role::Primary {
            return Ok(conn.name.clone());
        }
    }

    Ok(UNKNOWN.to_string())
}

fn evict_resource(drbd_resource: &str, delay: u32, me: &str) -> Result<()> {
    println!("Evicting {}", drbd_resource);
    let mut primary = get_primary(drbd_resource)?;
    if primary == UNKNOWN {
        println!(
            "Sorry, resource state for '{}' unknown, ignoring",
            drbd_resource
        );
        return Ok(());
    }
    if primary != me {
        println!(
            "Active on '{}', nothing to do on this node, ignoring",
            primary,
        );
        return Ok(());
    }

    let target = promoter::escaped_services_target(drbd_resource);
    systemctl(vec!["mask".into(), "--runtime".into(), target.clone()])?;
    systemctl(vec!["daemon-reload".into()])?;
    systemctl_out_err(vec!["stop".into(), target], Stdio::inherit(), Stdio::null())?;

    let mut needs_newline = false;
    for i in (0..=delay).rev() {
        primary = get_primary(drbd_resource)?;
        if primary != UNKNOWN && primary != me {
            // a know host/peer
            break;
        }

        let s = if i != 0 {
            i.to_string() + ".."
        } else {
            i.to_string()
        };
        print!("{}", s);
        io::stdout().flush()?;
        needs_newline = true;
        if i != 0 {
            // no need to sleep on last iteration
            if TERMINATE.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_secs(1));
        }
    }
    if needs_newline {
        println!("");
    }

    if primary == UNKNOWN {
        println!("Unfortunately no other node took over, resource in unknown state");
    } else if primary == me {
        println!("Unfortunately no other node took over, local node still DRBD Primary");
    } else {
        println!("Node '{}' took over", primary);
    }

    Ok(())
}

fn evict_resources(drbd_resources: &Vec<String>, keep_masked: bool, delay: u32) -> Result<()> {
    let me = promoter::uname_n()?;

    TERMINATE.store(false, Ordering::Relaxed);
    for drbd_res in drbd_resources {
        let result = evict_resource(drbd_res, delay, &me);
        if !keep_masked {
            evict_unmask_and_start(&vec![drbd_res.clone()])?;
        }
        result?;

        if TERMINATE.load(Ordering::Relaxed) {
            break;
        }
    }
    Ok(())
}

fn nr_plugins(plugins: &plugin::PluginConfig) -> usize {
    plugins.promoter.len() + plugins.umh.len() + plugins.debugger.len() + plugins.prometheus.len()
}

fn evict(
    snippets_paths: Vec<PathBuf>,
    force: bool,
    keep_masked: bool,
    unmask: bool,
    delay: u32,
) -> Result<()> {
    let mut drbd_resources = Vec::new();
    for snippet in snippets_paths {
        if !snippet.exists() {
            warn(&format!(
                "'{}' does not exist, doing nothing",
                snippet.display()
            ));
            continue;
        }
        let conf = read_config(&snippet)?;
        let plugins = conf.plugins;

        let nr_promoters = plugins.promoter.len();
        if nr_promoters == 0 {
            continue;
        }
        println!("{}:", snippet.display());

        if nr_plugins(&plugins) != nr_promoters && !force {
            return Err(anyhow::anyhow!(
                "Config file '{}' contains mixed promoter and other plugins",
                snippet.display()
            ));
        }

        for promoter in plugins.promoter {
            let res_names = promoter.resources.keys();
            if res_names.len() > 1 && !force {
                return Err(anyhow::anyhow!(
                    "Promoter in config file '{}' responsible for multiple resources",
                    snippet.display()
                ));
            }
            for (name, config) in promoter.resources {
                // sort by failure-action: The ones not potentially causing a reboot first
                if config.on_drbd_demote_failure == promoter::SystemdFailureAction::None {
                    drbd_resources.insert(0, name.clone());
                } else {
                    drbd_resources.push(name.clone());
                }
            }
        }
    }

    if unmask {
        evict_unmask_and_start(&drbd_resources)
    } else {
        evict_resources(&drbd_resources, keep_masked, delay)
    }
}

fn ls(snippets_paths: Vec<PathBuf>) -> Result<()> {
    for snippet in snippets_paths {
        println!("{}:", snippet.display());
        if !snippet.exists() {
            warn(&format!(
                "'{}' does not exist, doing nothing",
                snippet.display()
            ));
            continue;
        }
        let conf = read_config(&snippet)?;
        let plugins = conf.plugins;
        for promoter in plugins.promoter {
            print_promoter_id(&promoter);
        }
        for prometheus in plugins.prometheus {
            print_prometheus_id(&prometheus);
        }
        for debugger in plugins.debugger {
            print_debugger_id(&debugger);
        }
        for umh in plugins.umh {
            print_umh_id(&umh);
        }
    }

    Ok(())
}

fn restart(snippets_paths: Vec<PathBuf>, with_targets: bool) -> Result<()> {
    if snippets_paths.is_empty() {
        systemctl(vec!["restart".into(), REACTOR_SERVICE.into()])
    } else {
        disable(snippets_paths.clone(), with_targets)?;
        enable(
            snippets_paths
                .into_iter()
                .map(|p| get_disabled_path(&p))
                .collect(),
        )
    }
}

fn read_config(snippet_path: &PathBuf) -> Result<config::Config> {
    let content = config::read_snippets(&vec![snippet_path.clone()])
        .with_context(|| format!("Could not read config snippets"))?;
    let config = toml::from_str(&content).with_context(|| {
        format!(
            "Could not parse config files including snippets; content: {}",
            content
        )
    })?;

    Ok(config)
}

fn get_snippets_path(path: &PathBuf) -> Option<PathBuf> {
    let content = fs::read_to_string(path).ok()?;

    toml::from_str::<config::Config>(&content)
        .map(|c| c.snippets)
        .ok()?
}

fn expand_snippets(snippets_path: &PathBuf, matches: &ArgMatches, disabled: bool) -> Vec<PathBuf> {
    let expected_extension = match disabled {
        true => "toml.disabled",
        false => "toml",
    };

    let configs: Vec<PathBuf> = match matches.values_of("configs") {
        Some(configs) => configs.map(PathBuf::from).collect::<Vec<_>>(), // process them in the next stage
        None => {
            // "glob expand"
            match config::files_with_extension_in(snippets_path, expected_extension) {
                Ok(paths) => return paths,
                Err(e) => {
                    eprintln!(
                        "Error reading files in '{}': {}",
                        snippets_path.display(),
                        e
                    );
                    return Vec::new();
                }
            }
        }
    };

    let mut paths = Vec::new();
    for config in configs {
        if config.is_absolute() {
            paths.push(config);
            continue;
        }

        // not absolute
        let config = match config.extension() {
            None => config.with_extension(expected_extension),
            Some(extension) => {
                if extension == expected_extension {
                    config
                } else {
                    eprintln!(
                        "File '{}' has an extension, but it is not the expected one ('.{}'), ignoring",
                        config.display(),
                        expected_extension
                    );
                    continue;
                }
            }
        };

        let mut abspath = PathBuf::from(snippets_path);
        abspath.push(config);
        paths.push(abspath);
    }

    paths
}

fn get_app() -> App<'static, 'static> {
    App::new("drbd-reactorctl")
        .author(crate_authors!("\n"))
        .version(crate_version!())
        .about("Controls a local drbd-reactor daemon")
				.setting(AppSettings::VersionlessSubcommands)
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("Sets a custom config file")
                .default_value("/etc/drbd-reactor.toml"),
        )
        .subcommand(
            SubCommand::with_name("disable")
                .about("Disable plugin")
                .arg(
                    Arg::with_name("now")
                        .long("now")
                        .help("In case of promoter plugin stop the drbd-resources target"),
                )
                .arg(
                    Arg::with_name("configs")
                        .help("Configs to disable")
                        .required(false)
                        .multiple(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("enable").about("enable plugin").arg(
                Arg::with_name("configs")
                    .help("Configs to enable")
                    .required(false)
                    .multiple(true),
            ),
        )
        .subcommand(
            SubCommand::with_name("status")
                .about("Status informatin for a plugin")
                .arg(
                    Arg::with_name("verbose")
                        .help("Verbose output")
                        .short("v")
                        .long("verbose"),
                )
                .arg(
                    Arg::with_name("resource")
                        .help("In case of a promoter plugin limit to these DRBD resources")
                        .short("r")
                        .long("resource")
                        .multiple(true)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("configs")
                        .help("Configs to enable")
                        .required(false)
                        .multiple(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("restart")
                .about("Restart a plugin")
                .arg(
                    Arg::with_name("with_targets")
                        .long("with-targets")
                        .help("also stop the drbd-service@.target for promoter plugins, might get started on different node."),
                )
                .arg(
                    Arg::with_name("configs")
                        .help("Configs to restart")
                        .required(false)
                        .multiple(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("edit")
                .about("Edit/Add a plugin configuration")
                .arg(
                    Arg::with_name("type")
                        .short("t")
                        .long("type")
                        .help("Plugin type")
                        .takes_value(true)
                        .possible_values(&["promoter", "prometheus", "umh", "debugger"])
                        .default_value("promoter"),
                )
                .arg(
                    Arg::with_name("force")
                        .short("f")
                        .long("force")
                        .help("Force dangerous edits"),
                )
                .arg(
                    Arg::with_name("disabled")
                        .long("disabled")
                        .help("Edit a disabled file"),
                )
                .arg(
                    Arg::with_name("configs")
                        .help("Configs to edit/add")
                        .required(false)
                        .multiple(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("rm")
                .about("Remove plugin configuration")
                .arg(
                    Arg::with_name("disabled")
                        .long("disabled")
                        .help("Remove a disabled file"),
                )
                .arg(
                    Arg::with_name("force")
                        .short("f")
                        .long("force")
                        .help("Force"),
                )
                .arg(
                    Arg::with_name("configs")
                        .help("Configs to remove")
                        .required(true)
                        .multiple(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("evict")
                .about("Evict a promoter plugin controlled resource")
                .arg(
                    Arg::with_name("delay")
                        .short("d")
                        .long("delay")
                        .default_value("20")
                        .validator(has_positive_u32)
                        .help("Positive number of seconds to wait for peer takeover"),
                )
                .arg(
                    Arg::with_name("force")
                        .short("f")
                        .long("force")
                        .help("Override checks (multiple plugins per snippet/multiple resources per promoter)"),
                )
                .arg(
                    Arg::with_name("keep_masked")
                        .short("k")
                        .long("keep-masked")
                        .help("If set the target unit will stay masked (i.e., 'systemctl mask --runtime')"),
                )
                .arg(
                    Arg::with_name("unmask")
                        .short("u")
                        .long("unmask")
                        .long_help(
"If set unmask targets (i.e. the equivalent of 'systemctl unmask').
This does not run any evictions.
It is used to clear previous '--keep-masked' operations"),
                )
                .arg(
                    Arg::with_name("configs")
                        .help("Configs to remove")
                        .multiple(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("cat")
                .about("(Pretty) print config files")
                .arg(
                    Arg::with_name("configs")
                        .help("Configs to cat")
                        .multiple(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("ls")
                .about("list absolute path and ID of plugins")
                .arg(
                    Arg::with_name("disabled").long("disabled")
                        .help("show disabled plugins")
                )
                .arg(
                    Arg::with_name("configs")
                        .help("Configs to list")
                        .multiple(true)
                        .takes_value(true)
                ),
        )
        .subcommand(
            SubCommand::with_name("generate-completion")
                .about("Generate tab-complition for shell")
                .arg(
                    Arg::with_name("shell")
                        .help("Shell")
                        .takes_value(true)
												.required(true)
                        .possible_values(&Shell::variants())
                        .default_value("bash"),
								).display_order(1000),
				)
}

fn has_positive_u32(s: String) -> Result<(), String> {
    match s.parse::<u32>() {
        Ok(i) => {
            if i > 0 {
                Ok(())
            } else {
                Err(String::from("Value has to be a positive integer"))
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

fn systemctl_out_err(args: Vec<String>, stdout: Stdio, stderr: Stdio) -> Result<()> {
    plugin::map_status(
        Command::new("systemctl")
            .args(&args)
            .stdout(stdout)
            .stderr(stderr)
            .status(),
    )
}

fn systemctl(args: Vec<String>) -> Result<()> {
    systemctl_out_err(args, Stdio::inherit(), Stdio::inherit())
}

fn prometheus_connect(addr: &SocketAddr) -> Result<()> {
    let mut status = TcpStream::connect_timeout(&addr, Duration::from_secs(2));
    if status.is_ok() {
        return Ok(());
    }

    if addr.is_ipv6() && addr.ip().is_unspecified() {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, addr.port()));
        status = TcpStream::connect_timeout(&addr, Duration::from_secs(2));
    }

    match status {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow::anyhow!("{}", e)),
    }
}

fn show_property(unit: &str, property: &str) -> Result<String> {
    let output = Command::new("systemctl")
        .arg("show")
        .arg(format!("--property={}", property))
        .arg(unit)
        .output()?;
    let output = std::str::from_utf8(&output.stdout)?;
    // split_once('=') would be more elegant, but we want to support old rustc (e.g., bullseye)
    let mut split = output.splitn(2, '=');
    match (split.next(), split.next()) {
        (Some(k), Some(v)) if k == property => Ok(v.trim().to_string()),
        (Some(_), Some(_)) => Err(anyhow::anyhow!(
            "Property did not start with '{}='",
            property
        )),
        _ => Err(anyhow::anyhow!("Could not get property '{}'", property)),
    }
}

fn status_dot(unit: &str) -> Result<String> {
    let prop = show_property(unit, "ActiveState")?;
    let state = UnitActiveState::from_str(&prop)?;
    Ok(format!("{}", state))
}

fn freezer_state(unit: &str) -> Result<String> {
    // we can not always expect a value on older systemd that did not have freeze support
    // in that case we get an Err() which we discard.
    let prop = match show_property(unit, "FreezerState") {
        Ok(x) => x,
        Err(_) => return Ok("".into()),
    };
    let state = UnitFreezerState::from_str(&prop)?;
    Ok(format!("{}", state))
}

// most of that inspired by systemc/src/basic/unit-def.c
enum UnitFreezerState {
    Running,
    Freezing,
    Frozen,
    Thawing,
}
impl FromStr for UnitFreezerState {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Error> {
        match input {
            "running" => Ok(Self::Running),
            "freezing" => Ok(Self::Freezing),
            "frozen" => Ok(Self::Frozen),
            "thawing" => Ok(Self::Thawing),
            _ => Err(Error::new(
                ErrorKind::InvalidData,
                "unknown systemd FreezerState",
            )),
        }
    }
}
//  this is the opinonated version already discarding running
impl fmt::Display for UnitFreezerState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Running => write!(f, ""),
            Self::Freezing => write!(f, "({})", "freezing".blue()),
            Self::Frozen => write!(f, "({})", "frozen".blue()),
            Self::Thawing => write!(f, "(thawing)"),
        }
    }
}

// most of that inspired by systemc/src/basic/unit-def.c
enum UnitActiveState {
    Active,
    Reloading,
    Inactive,
    Failed,
    Activating,
    Deactivating,
    Maintenance,
}
impl FromStr for UnitActiveState {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Error> {
        match input {
            "active" => Ok(Self::Active),
            "reloading" => Ok(Self::Reloading),
            "inactive" => Ok(Self::Inactive),
            "failed" => Ok(Self::Failed),
            "activating" => Ok(Self::Activating),
            "deactivating" => Ok(Self::Deactivating),
            "maintenance" => Ok(Self::Maintenance),
            _ => Err(Error::new(
                ErrorKind::InvalidData,
                "unknown systemd ActiveState",
            )),
        }
    }
}
impl fmt::Display for UnitActiveState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Active => write!(f, "{}", "●".bold().green()),
            Self::Reloading => write!(f, "{}", "↻".bold().green()),
            Self::Inactive => write!(f, "{}", "○"),
            Self::Failed => write!(f, "{}", "×".bold().red()),
            Self::Activating => write!(f, "{}", "●".bold()),
            Self::Deactivating => write!(f, "{}", "●".bold()),
            Self::Maintenance => write!(f, "{}", "○"),
        }
    }
}

fn print_promoter_id(promoter: &plugin::promoter::PromoterConfig) {
    let id = match &promoter.id {
        Some(id) => id.clone(),
        None => "<none>".to_string(),
    };
    green(&format!("Promoter (ID: '{}')", id))
}

fn print_prometheus_id(prometheus: &plugin::prometheus::PrometheusConfig) {
    let id = match &prometheus.id {
        Some(id) => id.clone(),
        None => "<none>".to_string(),
    };
    green(&format!("Prometheus (ID: '{}')", id))
}

fn print_umh_id(umh: &plugin::umh::UMHConfig) {
    let id = match &umh.id {
        Some(id) => id.clone(),
        None => "<none>".to_string(),
    };
    green(&format!("UMH (ID: '{}')", id))
}

fn print_debugger_id(debugger: &plugin::debugger::DebuggerConfig) {
    let id = match &debugger.id {
        Some(id) => id.clone(),
        None => "<none>".to_string(),
    };
    green(&format!("Debugger (ID: '{}')", id))
}

fn green(text: &str) {
    println!("{}", text.bold().green())
}

fn warn(text: &str) {
    println!("{} {}", "WARN:".bold().yellow(), text)
}

const PROMOTER_TEMPLATE: &str = r###"[[promoter]]
id = "{id}"
[promoter.resources.$resname]
start = ["$service.mount", "$service.service"]
# runner = "systemd"
## if unset/empty, services from 'start' will be stopped in reverse order if runner is shell
## if runner is sytemd it just stops the implicitly generated systemd.target
# stop = []
# on-drbd-demote-failure = "reboot"
# stop-services-on-exit = false
#
# for more complex setups like HA iSCSI targets, NFS exports, or NVMe-oF targets consider
# https://github.com/LINBIT/linstor-gateway which uses LINSTOR and drbd-reactor"###;

const PROMETHEUS_TEMPLATE: &str = r###"[[prometheus]]
id = "prometheus"  # usually there is only one, this id should be fine
enums = true
# address = "[::]:9942""###;

const UMH_TEMPLATE: &str = r###"[[umh]]
id = "{id}"
[[umh.resource]]
command = "slack.sh $DRBD_RES_NAME on $(uname -n) from $DRBD_OLD_ROLE to $DRBD_NEW_ROLE"
event-type = "Change"
old.role = \{ operator = "NotEquals", value = "Primary" }
new.role = "Primary"
# This is a trivial resource rule example, please see drbd-reactor.umh(5) for more examples"###;

const DEBUGGER_TEMPLATE: &str = r###"[[debugger]]
id = "debugger"  # usually there is only one, id should be fine
# NOTE: make sure the log level in your [[log]] section is at least on level 'debug'"###;

fn generate_template(template: &str, id: &str) -> Result<String> {
    let mut tt = TinyTemplate::new();
    tt.add_template("template", template)?;

    #[derive(Serialize)]
    struct Context {
        id: String,
    }
    let result = tt.render("template", &Context { id: id.into() })?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basename_without_ext() {
        let bn = basename_without_ext(&PathBuf::from("/x/foo.toml"))
            .expect("getting basename from /x/foo.toml");
        assert_eq!(bn, PathBuf::from("foo"));

        let bn = basename_without_ext(&PathBuf::from("/x/foo.toml.disabled"))
            .expect("getting basename from /x/foo.toml.disabled");
        assert_eq!(bn, PathBuf::from("foo"));

        let bn = basename_without_ext(&PathBuf::from("/x/foo.bar"));
        assert!(bn.is_err());
    }
}
