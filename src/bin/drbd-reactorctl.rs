use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt;
use std::fmt::Write as fmtWrite;
use std::fs;
use std::io::{self, Write};
use std::io::{BufRead, BufReader};
use std::io::{Error, ErrorKind};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{crate_version, App, AppSettings, Arg, ArgMatches, Shell, SubCommand};
use colored::Colorize;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use signal_hook::iterator::Signals;
use tempfile::NamedTempFile;

use drbd_reactor::config;
use drbd_reactor::drbd;
use drbd_reactor::drbd::PrimaryOn;
use drbd_reactor::plugin;
use drbd_reactor::plugin::promoter;
use drbd_reactor::systemd;
use drbd_reactor::systemd::UnitActiveState;
use drbd_reactor::utils;

static TERMINATE: AtomicBool = AtomicBool::new(false);

const REACTOR_RELOAD_PATH: &str = "drbd-reactor-reload.path";
const REACTOR_SERVICE: &str = "drbd-reactor.service";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnippetState {
    Enabled,
    Disabled,
}

impl SnippetState {
    fn extension(&self) -> &'static str {
        match self {
            SnippetState::Enabled => ".toml",
            SnippetState::Disabled => ".toml.disabled",
        }
    }

    fn opposite(&self) -> SnippetState {
        match self {
            SnippetState::Enabled => SnippetState::Disabled,
            SnippetState::Disabled => SnippetState::Enabled,
        }
    }
}

/// Determine the snippet state from a path's extension using string matching.
/// Checks `.toml.disabled` before `.toml` since the former also ends with the latter.
fn snippet_state_of(path: &Path) -> Option<SnippetState> {
    let s = path.to_str()?;
    if s.ends_with(".toml.disabled") {
        Some(SnippetState::Disabled)
    } else if s.ends_with(".toml") {
        Some(SnippetState::Enabled)
    } else {
        None
    }
}

/// Replace or append the snippet extension on a path.
fn with_snippet_extension(path: &Path, state: SnippetState) -> PathBuf {
    let s = path.to_str().expect("path must be valid UTF-8");
    let base = if let Some(stripped) = s.strip_suffix(".toml.disabled") {
        stripped
    } else if let Some(stripped) = s.strip_suffix(".toml") {
        stripped
    } else {
        s
    };
    PathBuf::from(format!("{}{}", base, state.extension()))
}

/// Get the "opposite" path: foo.toml <-> foo.toml.disabled
fn opposite_snippet_path(path: &Path) -> Option<PathBuf> {
    let current = snippet_state_of(path)?;
    Some(with_snippet_extension(path, current.opposite()))
}

fn enabled_str(enabled: bool) -> &'static str {
    if enabled {
        ""
    } else {
        " (disabled)"
    }
}

struct ClusterConf<'a> {
    context: &'a str,
    nodes: Vec<&'a str>,
    local: bool,
}

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

    let context = matches
        .value_of("context")
        .expect("expected to have a default");

    let nodes = matches
        .values_of("nodes")
        .expect("expected to have a default")
        .filter(|&x| !x.is_empty())
        .collect::<Vec<_>>();

    let local = matches.is_present("local");

    let cluster = ClusterConf {
        context,
        nodes,
        local,
    };

    match matches.subcommand() {
        ("cat", Some(m)) => cat(
            snippets_from_matches(&snippets_path, m, SnippetState::Enabled, true),
            &cluster,
        ),
        ("disable", Some(m)) => {
            let now = m.is_present("now");
            disable(
                snippets_from_matches(&snippets_path, m, SnippetState::Enabled, true),
                now,
                &cluster,
            )
        }
        ("enable", Some(m)) => enable(
            snippets_from_matches(&snippets_path, m, SnippetState::Disabled, true),
            &cluster,
        ),
        ("edit", Some(m)) => {
            let state = if m.is_present("disabled") {
                SnippetState::Disabled
            } else {
                SnippetState::Enabled
            };
            let force = m.is_present("force");
            let type_opt = m.value_of("type").expect("expected to have a default");
            edit(
                snippets_from_matches(&snippets_path, m, state, false),
                &snippets_path,
                type_opt,
                force,
                &cluster,
            )
        }
        ("evict", Some(m)) => {
            let force = m.is_present("force");
            let keep_masked = m.is_present("keep_masked");
            let unmask = m.is_present("unmask");
            let delay = m.value_of("delay").expect("expected to have a default");
            let delay = delay.parse().expect("expected to be checked by parser");
            evict(
                snippets_from_matches(&snippets_path, m, SnippetState::Enabled, true),
                force,
                keep_masked,
                unmask,
                delay,
            )
        }
        ("ls", Some(m)) => {
            let cfgs: Option<Vec<String>> = m
                .values_of("configs")
                .map(|v| v.map(String::from).collect());
            let (enabled, disabled) = resolve_and_warn_both(&snippets_path, cfgs.as_deref());
            ls(enabled, disabled, &cluster)
        }
        ("restart", Some(m)) => {
            let with_targets = m.is_present("with_targets");
            let configs = match m.values_of("configs") {
                None => Vec::new(),
                Some(_) => snippets_from_matches(&snippets_path, m, SnippetState::Enabled, true),
            };
            restart(configs, with_targets, &cluster)
        }
        ("rm", Some(m)) => {
            let force = m.is_present("force");
            let state = if m.is_present("disabled") {
                SnippetState::Disabled
            } else {
                SnippetState::Enabled
            };
            rm(
                snippets_from_matches(&snippets_path, m, state, true),
                force,
                &cluster,
            )
        }
        ("start-until", Some(m)) => {
            let until = m
                .value_of("until")
                .expect("expected to be checked by parser");
            start_until(
                snippets_from_matches(&snippets_path, m, SnippetState::Disabled, true),
                until,
            )
        }
        ("status", Some(m)) => {
            let verbose = m.is_present("verbose");
            let format = match m.is_present("json") {
                true => Format::Json,
                false => Format::Terminal,
            };
            let resources = m.values_of("resource").unwrap_or_default();
            let resources: Vec<String> = resources.map(String::from).collect::<Vec<_>>();
            let cfgs: Option<Vec<String>> = m
                .values_of("configs")
                .map(|v| v.map(String::from).collect());
            let (enabled, disabled) = resolve_and_warn_both(&snippets_path, cfgs.as_deref());
            status(enabled, disabled, &resources, &cluster)
                .and_then(|s| Ok(print!("{}", s.format(&format, verbose)?)))
        }
        _ => {
            // pretend it is status
            let (enabled, disabled) = resolve_and_warn_both(&snippets_path, None);
            status(enabled, disabled, &vec![], &cluster)
                .and_then(|s| Ok(print!("{}", s.format(&Format::Terminal, false)?)))
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
            x => println!("Unknown answer '{x}', use 'y' or 'n'"),
        }
    }
}

fn edit_editor(tmppath: &Path, editor: &str, type_opt: &str, force: bool) -> Result<()> {
    let len_err =
        || -> Result<()> { Err(anyhow::anyhow!("Expected excactly one {type_opt} plugin")) };

    plugin::map_status(Command::new(editor).arg(tmppath).status())?;

    let content = fs::read_to_string(tmppath)?;
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
                        let err = "Mount unit should not be the topmost unit, consider using an \
                                   OCF file system RA";
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
    } else if type_opt == "agentx" {
        if plugins.agentx.len() != 1 {
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
        return Err(anyhow::anyhow!("Unknown type ('{type_opt}') to edit"));
    }

    Ok(())
}

fn add_header(tmppath: &Path, last_result: &Result<()>) -> Result<()> {
    let was = fs::read_to_string(tmppath)?;
    let mut f = fs::File::create(tmppath)?;
    writeln!(
        &mut f,
        "#| Please edit the snippet below. Lines beginning with a '#|' will be ignored,"
    )?;
    writeln!(
        &mut f,
        "#| and an empty file will abort the edit. If an error occurs while saving this file will \
         be"
    )?;
    writeln!(&mut f, "#| reopened with the relevant failures.")?;
    writeln!(&mut f, "#|")?;
    if let Err(e) = last_result {
        writeln!(&mut f, "#| Error: {}", e)?;
    } else {
        writeln!(&mut f, "#| Happy editing:")?;
    }
    write!(&mut f, "{}", was)?;
    Ok(())
}

fn rm_header(tmppath: &Path) -> Result<()> {
    let lines: Vec<String> = BufReader::new(fs::File::open(tmppath)?)
        .lines()
        .collect::<std::result::Result<_, _>>()?;

    let mut file = fs::File::create(tmppath)?;
    for line in lines.into_iter().filter(|l| !l.starts_with("#|")) {
        file.write_all(line.as_bytes())?;
        file.write_all("\n".as_bytes())?;
    }

    Ok(())
}

fn edit(
    snippets_paths: Vec<PathBuf>,
    snippets_path: &PathBuf,
    type_opt: &str,
    force: bool,
    cluster: &ClusterConf,
) -> Result<()> {
    if do_remote(cluster)? {
        return Ok(());
    }

    let editor = env::var("EDITOR").unwrap_or("vi".to_string());

    let mut persisted = 0;
    for snippet in &snippets_paths {
        // use new_in() to avoid $TMPDIR being on a different mount point than snippets_path
        // as this would result in an error on .persist()
        // also we can avoid using special methods and can just use the path as there won't be any TMPDIR cleaners
        let mut tmpfile = NamedTempFile::new_in(snippets_path)?;
        let mut from_template = false;
        if snippet.exists() {
            fs::copy(snippet, tmpfile.path())?;
        } else {
            let template = match type_opt {
                "promoter" => PROMOTER_TEMPLATE,
                "prometheus" => PROMETHEUS_TEMPLATE,
                "agentx" => AGENTX_TEMPLATE,
                "umh" => UMH_TEMPLATE,
                "debugger" => DEBUGGER_TEMPLATE,
                x => return Err(anyhow::anyhow!("Unknown type ('{x}') to edit")),
            };
            tmpfile.write_all(template.as_bytes())?;
            tmpfile.flush()?;
            from_template = true;
        }

        let mut aborted = false;
        let mut result: Result<()> = Ok(());
        loop {
            // be careful on first iteration, consider it to be empty
            let was = if from_template {
                from_template = false;
                "".to_string()
            } else {
                fs::read_to_string(tmpfile.path())?
            };
            let was = was.trim();
            add_header(tmpfile.path(), &result)?;
            result = edit_editor(tmpfile.path(), &editor, type_opt, force);
            rm_header(tmpfile.path())?;
            let is = fs::read_to_string(tmpfile.path())?;
            let is = is.trim();

            if is.is_empty() {
                warn("Edit aborted, empty file saved");
                aborted = true;
                break;
            } else if was == is {
                warn("Edit aborted, no new changes have been made");
                aborted = true;
                break;
            }

            if result.is_ok() {
                break;
            }
        }

        if !aborted {
            tmpfile.persist(snippet)?;
            persisted += 1;
        }
    }

    if persisted > 0 && !has_autoload()? {
        reload_service()?;
    }

    Ok(())
}

fn rm(snippets_paths: Vec<PathBuf>, force: bool, cluster: &ClusterConf) -> Result<()> {
    if do_remote(cluster)? {
        return Ok(());
    }

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

fn start_until_list(config: promoter::PromoterOptResource, until: &str) -> Result<Vec<String>> {
    match until.parse::<usize>() {
        Ok(n) => Ok(config.start.into_iter().take(n).collect()),
        Err(_) => {
            // assume it it is a service name
            match config.start.iter().position(|s| s == until) {
                Some(n) => Ok(config.start.into_iter().take(n + 1).collect()),
                None => Err(anyhow::anyhow!(
                    "Could not find unit '{until}' in start list"
                )),
            }
        }
    }
}

fn start_until(snippets_paths: Vec<PathBuf>, until: &str) -> Result<()> {
    if snippets_paths.is_empty() {
        return Err(anyhow::anyhow!("Could not get disabled snippet file"));
    }
    let path = &snippets_paths[0];
    let conf = read_config(path)
        .map_err(|_| anyhow::anyhow!("File '{}' does not exist", path.display()))?;
    for promoter in conf.plugins.promoter {
        // generate the target and therefore all overrides
        let _ = promoter::Promoter::new(promoter.clone())?;
        for (drbd_res, config) in promoter.resources {
            let start_list = start_until_list(config, until)?;
            let promote_service = promote_service(&drbd_res);
            println!("systemctl start {}", promote_service);
            systemctl(vec!["start".into(), promote_service])?;
            for start in start_list {
                let service_name = service_name(&start, &drbd_res)?;
                println!("systemctl start {}", service_name);
                systemctl(vec!["start".into(), service_name])?;
            }
            info("To resume normal operation, execute:");
            println!(
                "- systemctl start {} # on this node",
                systemd::escaped_services_target(&drbd_res)
            );
            println!(
                "- drbd-reactorctl enable {} # on all cluster nodes",
                path.display()
            );
        }
    }
    Ok(())
}

fn enable(snippets_paths: Vec<PathBuf>, cluster: &ClusterConf) -> Result<()> {
    if do_remote(cluster)? {
        return Ok(());
    }

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
        let enabled_path = get_enabled_path(snippet)?;
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
        let conf = read_config(snippet)?;
        for promoter in conf.plugins.promoter {
            for drbd_res in promoter.resources.keys() {
                let target = systemd::escaped_services_target(drbd_res);
                systemctl(vec!["stop".into(), target])?;
            }
        }
    }

    Ok(())
}

fn disable(snippets_paths: Vec<PathBuf>, with_targets: bool, cluster: &ClusterConf) -> Result<()> {
    if do_remote(cluster)? {
        return Ok(());
    }

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
        let disabled_path = get_disabled_path(snippet);
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

fn get_disabled_path(snippet_path: &Path) -> PathBuf {
    with_snippet_extension(snippet_path, SnippetState::Disabled)
}

fn get_enabled_path(snippet_path: &Path) -> Result<PathBuf> {
    match snippet_state_of(snippet_path) {
        Some(SnippetState::Disabled) => {
            Ok(with_snippet_extension(snippet_path, SnippetState::Enabled))
        }
        Some(SnippetState::Enabled) => Err(anyhow::anyhow!(
            "Expected plugin path '{}' to end in .toml.disabled",
            snippet_path.display()
        )),
        None => Err(anyhow::anyhow!(
            "Expected to get proper extension for plugin path '{}'",
            snippet_path.display()
        )),
    }
}

fn has_autoload() -> Result<bool> {
    let status = Command::new("systemctl")
        .arg("is-active")
        .arg("-q")
        .arg(REACTOR_RELOAD_PATH)
        .status()
        .with_context(|| "Could not execute 'systemctl'")?;
    Ok(status.success())
}

fn reload_service() -> Result<()> {
    systemctl(vec!["reload".into(), REACTOR_SERVICE.into()])
}

fn tagged_snippets<'a, T>(
    enabled: &'a [T],
    disabled: &'a [T],
) -> impl Iterator<Item = (bool, &'a T)> {
    enabled
        .iter()
        .map(|x| (true, x))
        .chain(disabled.iter().map(|x| (false, x)))
}

fn status(
    enabled_snippets_paths: Vec<PathBuf>,
    disabled_snippets_paths: Vec<PathBuf>,
    resources: &Vec<String>,
    cluster: &ClusterConf,
) -> Result<Status> {
    let mut status = Status {
        ..Default::default()
    };
    if do_remote(cluster)? {
        return Ok(status);
    }

    for (enabled, snippet) in tagged_snippets(&enabled_snippets_paths, &disabled_snippets_paths) {
        let conf = read_config(&snippet)?;
        let plugins = conf.plugins;
        for promoter in plugins.promoter {
            for (drbd_res, config) in promoter.resources {
                // check if in filter
                if !resources.is_empty() && !resources.contains(&drbd_res) {
                    continue;
                }
                let target = systemd::escaped_services_target(&drbd_res);
                let primary_on = drbd::get_primary(&drbd_res)?;
                // target itself and the implicit one
                let promote_service = promote_service(&drbd_res);
                let mut dependencies = Vec::new();
                let target = SystemdUnit::from_str(&target)?;
                dependencies.push(SystemdUnit::from_str(&promote_service)?);
                let state = target.status.clone();

                for start in config.start {
                    let service_name = service_name(&start, &drbd_res)?;
                    dependencies.push(SystemdUnit::from_str(&service_name)?);
                }

                status.promoter.push(PromoterStatus {
                    drbd_resource: drbd_res.clone(),
                    path: snippet.clone(),
                    enabled,
                    primary_on,
                    target,
                    dependencies,
                    status: state,
                });
            }
        }
        for prometheus in plugins.prometheus {
            status.prometheus.push(PrometheusStatus {
                path: snippet.clone(),
                enabled,
                address: prometheus.address.clone(),
                status: UnitActiveState::Active,
            })
        }
        for _ in plugins.debugger {
            status.debugger.push(DebuggerStatus {
                path: snippet.clone(),
                enabled,
                status: UnitActiveState::Active,
            })
        }
        for _ in plugins.umh {
            status.umh.push(UMHStatus {
                path: snippet.clone(),
                enabled,
                status: UnitActiveState::Active,
            })
        }
        for agentx in plugins.agentx {
            status.agentx.push(AgentXStatus {
                path: snippet.clone(),
                enabled,
                address: agentx.address.clone(),
                status: UnitActiveState::Active,
            })
        }
    }
    Ok(status)
}

fn cat(snippets_paths: Vec<PathBuf>, cluster: &ClusterConf) -> Result<()> {
    if do_remote(cluster)? {
        return Ok(());
    }
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
        let target = systemd::escaped_services_target(drbd_res);
        println!("Re-enabling {}", drbd_res);

        // old (at least RHEL8) systemctl allows you to mask --runtime, but does not allow unmask --runtime
        // we know that we created the thing via mask
        let path = "/run/systemd/system/".to_owned() + &target;
        match fs::remove_file(Path::new(&path)) {
            Ok(()) => (),
            Err(e) if e.kind() == ErrorKind::NotFound => (), // Target was never masked to begin with
            Err(e) => Err(e)?,
        };

        println!("Removed {}.", path); // like systemctl unmask would print it
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

fn evict_resource(drbd_resource: &str, delay: u32) -> Result<()> {
    println!("Evicting {}", drbd_resource);
    match drbd::get_primary(drbd_resource)? {
        PrimaryOn::None => {
            println!(
                "Sorry, resource state for '{}' unknown, ignoring",
                drbd_resource
            );
            return Ok(());
        }
        PrimaryOn::Remote(r) => {
            println!("Active on '{}', nothing to do on this node, ignoring", r,);
            return Ok(());
        }
        PrimaryOn::Local(_) => (), // we continue
    };

    let target = systemd::escaped_services_target(drbd_resource);
    systemctl(vec!["mask".into(), "--runtime".into(), target.clone()])?;
    systemctl(vec!["daemon-reload".into()])?;
    systemctl_out_err(vec!["stop".into(), target], Stdio::inherit(), Stdio::null())?;

    let mut needs_newline = false;
    for i in (0..=delay).rev() {
        // a know host/peer?
        if let PrimaryOn::Remote(_r) = drbd::get_primary(drbd_resource)? {
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
        println!();
    }

    match drbd::get_primary(drbd_resource)? {
        PrimaryOn::Local(_) => {
            println!("Local node still DRBD Primary, not all services stopped in time locally");
        }
        PrimaryOn::Remote(r) => println!("Node '{}' took over", r),
        PrimaryOn::None => {
            println!("Unfortunately no other node took over, resource in unknown state")
        }
    };

    Ok(())
}

fn evict_resources(drbd_resources: &Vec<String>, keep_masked: bool, delay: u32) -> Result<()> {
    TERMINATE.store(false, Ordering::Relaxed);
    for drbd_res in drbd_resources {
        let result = evict_resource(drbd_res, delay);
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
    plugins.promoter.len()
        + plugins.umh.len()
        + plugins.debugger.len()
        + plugins.prometheus.len()
        + plugins.agentx.len()
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

fn ls(
    enabled_snippets_paths: Vec<PathBuf>,
    disabled_snippets_paths: Vec<PathBuf>,
    cluster: &ClusterConf,
) -> Result<()> {
    if do_remote(cluster)? {
        return Ok(());
    }

    for (enabled, snippet) in tagged_snippets(&enabled_snippets_paths, &disabled_snippets_paths) {
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
            for (drbd_res, resource) in promoter.resources {
                let mut start = resource.start.clone();
                if start.len() > 3 {
                    start.truncate(3);
                    start.push("...".into());
                }
                let single = start.iter().map(|s| s.len()).max().unwrap_or_default();
                let all: usize = start.iter().map(|s| s.len()).sum();
                // some very rough estimate...
                let delim = if single > 80 || all > 80 { "\n   " } else { "" };
                let state = enabled_str(enabled);
                print!("- Promoter: {}{}; start = [", drbd_res, state);
                for (i, s) in start.iter().enumerate() {
                    print!("{}\"{}\"", delim, s);
                    if i < start.len() - 1 {
                        print!(", ");
                    }
                }
                println!("]");
            }
        }
        for prometheus in plugins.prometheus {
            println!("- Prometheus: {}", prometheus.address);
        }
        for _ in plugins.debugger {
            println!("- Debugger");
        }
        for _ in plugins.umh {
            println!("- UMH");
        }
        for agentx in plugins.agentx {
            println!("- AgentX: {}", agentx.address);
        }
    }

    Ok(())
}

fn restart(snippets_paths: Vec<PathBuf>, with_targets: bool, cluster: &ClusterConf) -> Result<()> {
    if snippets_paths.is_empty() {
        systemctl(vec!["restart".into(), REACTOR_SERVICE.into()])
    } else {
        disable(snippets_paths.clone(), with_targets, cluster)?;
        enable(
            snippets_paths
                .into_iter()
                .map(|p| get_disabled_path(&p))
                .collect(),
            cluster,
        )
    }
}

fn read_config(snippet_path: &Path) -> Result<config::Config> {
    let content = config::read_snippets(&[snippet_path])?;
    let config = toml::from_str(&content).with_context(|| {
        format!("Could not parse config files including snippets; content: {content}")
    })?;

    Ok(config)
}

fn get_snippets_path(path: &PathBuf) -> Option<PathBuf> {
    let content = fs::read_to_string(path).ok()?;

    toml::from_str::<config::Config>(&content)
        .map(|c| c.snippets)
        .ok()?
}

fn resolve_snippets(
    snippets_path: &Path,
    configs: Option<&[String]>,
    state: SnippetState,
) -> Vec<PathBuf> {
    let configs = match configs {
        Some(c) => c,
        None => {
            match config::files_with_extension_in(
                &snippets_path.to_path_buf(),
                state.extension().trim_start_matches('.'),
            ) {
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
    for config_str in configs {
        let config = PathBuf::from(config_str);

        if config.is_absolute() {
            paths.push(config);
            continue;
        }

        // relative path: check for recognized extension, add one if missing
        let with_ext = match snippet_state_of(&config) {
            Some(_) => config,
            None => with_snippet_extension(&config, state),
        };

        let mut abspath = snippets_path.to_path_buf();
        abspath.push(with_ext);
        paths.push(abspath);
    }

    paths
}

/// Glue between clap's `ArgMatches` and the (clap-free, unit-tested) snippet
/// resolution core: extract the "configs" argument, resolve it against
/// `snippets_path` for `state`, and validate the result.
fn snippets_from_matches(
    snippets_path: &Path,
    matches: &ArgMatches,
    state: SnippetState,
    require_exists: bool,
) -> Vec<PathBuf> {
    let cfgs: Option<Vec<String>> = matches
        .values_of("configs")
        .map(|v| v.map(String::from).collect());
    let resolved = resolve_snippets(snippets_path, cfgs.as_deref(), state);
    validate_snippets(snippets_path, &resolved, require_exists)
}

/// Warn once for each snippet that has both its enabled (.toml) and disabled
/// (.toml.disabled) variant present on disk.
fn warn_if_both_exist<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) {
    let mut warned: HashSet<PathBuf> = HashSet::new();
    for path in paths {
        if let Some(opposite) = opposite_snippet_path(path) {
            if opposite.exists() && !warned.contains(path) && !warned.contains(&opposite) {
                warn(&format!(
                    "both '{}' and '{}' exist",
                    path.display(),
                    opposite.display()
                ));
                warned.insert(path.clone());
                warned.insert(opposite);
            }
        }
    }
}

fn validate_snippets(
    snippets_path: &Path,
    paths: &[PathBuf],
    require_exists: bool,
) -> Vec<PathBuf> {
    let mut valid = Vec::new();
    for path in paths {
        if snippet_state_of(path).is_none() {
            eprintln!(
                "File '{}' does not have a recognized extension (.toml or .toml.disabled), \
                 ignoring",
                path.display()
            );
            continue;
        }

        if !path.starts_with(snippets_path) {
            eprintln!(
                "File '{}' is not within snippets path '{}', ignoring",
                path.display(),
                snippets_path.display()
            );
            continue;
        }

        if require_exists && !path.is_file() {
            eprintln!(
                "File '{}' does not exist or is not a regular file, ignoring",
                path.display()
            );
            continue;
        }

        valid.push(path.clone());
    }
    warn_if_both_exist(&valid);
    valid
}

fn resolve_and_warn_both(
    snippets_path: &Path,
    configs: Option<&[String]>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let enabled = resolve_snippets(snippets_path, configs, SnippetState::Enabled);
    let disabled = resolve_snippets(snippets_path, configs, SnippetState::Disabled);

    warn_if_both_exist(enabled.iter().chain(disabled.iter()));

    (enabled, disabled)
}

fn promote_service(drbd_res: &str) -> String {
    format!("drbd-promote@{}.service", systemd::escape_name(drbd_res))
}

fn service_name(start_entry: &str, drbd_res: &str) -> Result<String> {
    let ocf_pattern = Regex::new(plugin::promoter::OCF_PATTERN)?;
    let start = start_entry.trim();
    let (service_name, _) = match ocf_pattern.captures(start) {
        Some(ocf) => {
            let (vendor, agent, args) = (&ocf[1], &ocf[2], &ocf[3]);
            systemd::escaped_ocf_parse_to_env(drbd_res, vendor, agent, args)?
        }
        _ => (start.to_string(), Vec::new()),
    };

    Ok(service_name)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Node {
    #[serde(default)]
    hostname: String,
    #[serde(default = "default_user")]
    user: String,
}
fn default_user() -> String {
    "root".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
struct Config {
    #[serde(default)]
    nodes_script: Option<String>,
    #[serde(default)]
    nodes: HashMap<String, Node>,
}

fn cfg_dir() -> Result<PathBuf> {
    let cfg_dir = match env::var("XDG_CONFIG_HOME") {
        Ok(x) if !x.is_empty() => PathBuf::from(x),
        _ => match env::var("HOME") {
            Ok(x) => Path::new(&x).join(".config"),
            Err(e) => return Err(anyhow::anyhow!(e)),
        },
    };
    Ok(cfg_dir.join("drbd-reactorctl"))
}

fn read_ctl_config(context: &str, additional_content: Option<&str>) -> Result<Config> {
    let cfg_file = cfg_dir()?.join(format!("{context}.toml"));

    // it is fine if the default.toml symlink does not exist
    if context == "default" && !cfg_file.exists() {
        return Ok(Default::default());
    }

    let mut content = fs::read_to_string(&cfg_file)
        .with_context(|| format!("Could not read config file: {}", cfg_file.display()))?;

    if let Some(ac) = additional_content {
        content.push('\n');
        content.push_str(ac);
    }

    toml::from_str(&content)
        .with_context(|| format!("Could not parse drbd-reactorctl config file; content: {content}"))
}

fn read_nodes(cluster: &ClusterConf) -> Result<Vec<Node>> {
    if cluster.context == "none" || cluster.context == "local" {
        return Ok(Vec::new());
    }

    let cfg = read_ctl_config(cluster.context, None)?;
    let cfg = match cfg.nodes_script {
        Some(script) => {
            let script = cfg_dir()?.join(script);
            let output = Command::new(&script)
                .output()
                .with_context(|| format!("Could not execute script '{}'", script.display()))?;
            if !output.status.success() {
                return Err(anyhow::anyhow!(
                    "Script '{}', did not return successfully",
                    script.display()
                ));
            }
            let output = String::from_utf8(output.stdout)?;
            read_ctl_config(cluster.context, Some(&output))?
        }
        None => cfg,
    };

    // "postparse": set hostname to nodename if not overwritten
    // filter if we got a limited set of nodes
    let mut nodes = Vec::new();
    for (name, node) in cfg.nodes {
        // it is slightly easier to filter here based on the "name" (i.e., "nick name")
        // than to filter "nodes" later, where we only have the expanded hostname
        if !cluster.nodes.is_empty() && !cluster.nodes.contains(&name.as_str()) {
            continue;
        }
        let mut node = node.clone();
        if node.hostname.is_empty() {
            node.hostname = name.clone();
        }
        nodes.push(node);
    }
    nodes.sort_by(|a, b| (a.hostname).cmp(&b.hostname));

    Ok(nodes)
}

fn pexec(cmds: &[Vec<String>]) -> Result<Vec<Output>> {
    let mut threads = Vec::with_capacity(cmds.len());
    for cmd in cmds {
        let process = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdin(Stdio::null())
            .spawn()
            .with_context(|| format!("Could not execute '{}'", cmd[0]))?;

        threads.push(thread::spawn(move || process.wait_with_output()));
    }

    threads
        .into_iter()
        .map(|t| {
            t.join()
                .expect("thread should not panic")
                .map_err(anyhow::Error::from)
        })
        .collect()
}

fn do_remote(cluster: &ClusterConf) -> Result<bool> {
    if cluster.local {
        return Ok(false);
    }

    let nodes = read_nodes(cluster)?;
    if nodes.is_empty() {
        return Ok(false);
    }

    // remote execution (except local node)
    let me = utils::uname_n_once();

    // check if we can reach all nodes, otherwise we might run into some inconsistent cluster state
    // that is obviously not a 100% guarantee, but IMO a check worth having
    print!("Checking ssh connection to all remote nodes: ");
    io::stdout().flush()?;
    let mut cmds = Vec::new();
    for node in &nodes {
        if node.hostname == *me {
            continue;
        }
        let userhost = format!("{}@{}", node.user, node.hostname);
        cmds.push(vec!["ssh".to_string(), userhost, "true".to_string()]);
    }
    let results = pexec(&cmds)?;
    for (i, result) in results.iter().enumerate() {
        if !result.status.success() {
            return Err(anyhow::anyhow!("Command '{}' failed", cmds[i].join(" ")));
        }
    }
    green("✓");

    let orig_args: Vec<String> = env::args().skip(1).collect();
    cmds.clear();
    for node in &nodes {
        let is_me = *me == node.hostname;
        let mut node_args = Vec::new();
        if !is_me {
            node_args.push("ssh".to_string());
            node_args.push("-qtt".to_string());
            node_args.push(format!("{}@{}", node.user, node.hostname));
            node_args.push("--".to_string());
        }
        node_args.push("drbd-reactorctl".to_string());
        node_args.push("--local".to_string());
        let mut args = orig_args.clone();
        node_args.append(&mut args);
        cmds.push(node_args);
    }
    let results = pexec(&cmds)?;
    for (i, result) in results.iter().enumerate() {
        if !result.status.success() {
            return Err(anyhow::anyhow!("Command '{}' failed", cmds[i].join(" ")));
        }
        println!(
            "➞ {}:\n{}",
            nodes[i].hostname,
            std::str::from_utf8(&result.stdout).unwrap_or("<Could not convert stdout>")
        );
    }

    Ok(true)
}

fn get_app() -> App<'static, 'static> {
    App::new("drbd-reactorctl")
        .author("Roland Kammerer <roland.kammerer@linbit.com>\nMoritz Wanzenböck <moritz.wanzenboeck@linbit.com>")
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
        .arg(
            Arg::with_name("context")
                .long("context")
                .help("Uses the given (cluster) context")
                .default_value("default")
                .global(true),
        )
        .arg(
            Arg::with_name("nodes")
                .long("nodes")
                .help("Uses only the given nodes from the context")
                .default_value("")
                .multiple(true)
                .require_delimiter(true)
                .global(true),
        )
        .arg(Arg::with_name("local").long("local").hidden(true))
        .subcommand(
            SubCommand::with_name("disable")
                .about("Disable plugin")
                .arg(
                    Arg::with_name("now")
                        .long("now")
                        .help("In case of promoter plugin stop the drbd-services target"),
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
                    Arg::with_name("json")
                        .help("Json output")
                        .long("json"),
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
                .arg(Arg::with_name("with_targets").long("with-targets").help(
                    "also stop the drbd-service@.target for promoter plugins, might get started \
                     on different node.",
                ))
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
                        .possible_values(&["promoter", "prometheus", "agentx", "umh", "debugger"])
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
                .arg(Arg::with_name("force").short("f").long("force").help(
                    "Override checks (multiple plugins per snippet/multiple resources per \
                     promoter)",
                ))
                .arg(
                    Arg::with_name("keep_masked")
                        .short("k")
                        .long("keep-masked")
                        .help(
                            "If set the target unit will stay masked (i.e., 'systemctl mask \
                             --runtime')",
                        ),
                )
                .arg(
                    Arg::with_name("unmask")
                        .short("u")
                        .long("unmask")
                        .long_help(
                            "If set unmask targets (i.e. the equivalent of 'systemctl unmask').
This does not run any evictions.
It is used to clear previous '--keep-masked' operations",
                        ),
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
                .about("List plugins and show some useful, brief information")
                .arg(
                    Arg::with_name("configs")
                        .help("Configs to list")
                        .multiple(true)
                        .takes_value(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("start-until")
                .about("Start reactor target until specified service in start list")
                .arg(Arg::with_name("until").required(true).help(
                    "Positive number or service name until which service in the start list the \
                     target should be started",
                ))
                .arg(
                    Arg::with_name("configs")
                        .help("Config to start")
                        .required(true)
                        .multiple(false),
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
                )
                .display_order(1000),
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

// most of that inspired by systemc/src/basic/unit-def.c
enum UnitFreezerState {
    Running,
    Freezing,
    Frozen,
    Thawing,
    Unknown,
}
impl Serialize for UnitFreezerState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
impl FromStr for UnitFreezerState {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Error> {
        match input {
            "running" => Ok(Self::Running),
            "freezing" => Ok(Self::Freezing),
            "frozen" => Ok(Self::Frozen),
            "thawing" => Ok(Self::Thawing),
            "unknown" => Ok(Self::Unknown),
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
            Self::Running => write!(f, "running"),
            Self::Freezing => write!(f, "freezing"),
            Self::Frozen => write!(f, "frozen"),
            Self::Thawing => write!(f, "thawing"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl UnitFreezerState {
    fn terminal(&self, _verbose: bool) -> Result<String> {
        Ok(match self {
            Self::Running => "".to_string(),
            Self::Freezing => "freezing".blue().to_string(),
            Self::Frozen => "frozen".blue().to_string(),
            Self::Thawing => "thawing".to_string(),
            Self::Unknown => "unknown".to_string(),
        })
    }
}

enum Format {
    Terminal,
    Json,
}
impl FromStr for Format {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Error> {
        match input {
            "text" => Ok(Self::Terminal),
            "json" => Ok(Self::Json),
            _ => Err(Error::new(ErrorKind::InvalidData, "unknown format")),
        }
    }
}

#[derive(Serialize)]
struct SystemdUnit {
    name: String,
    status: UnitActiveState,
    freezer: UnitFreezerState,
}
impl FromStr for SystemdUnit {
    type Err = Error;

    fn from_str(unit: &str) -> Result<Self, Error> {
        let prop = systemd::show_property(unit, "ActiveState")
            .map_err(|e| Error::new(ErrorKind::Other, e))?;
        let status = UnitActiveState::from_str(&prop)?;

        let prop = systemd::show_property(unit, "FreezerState").unwrap_or("unknown".to_string());
        let freezer = UnitFreezerState::from_str(&prop)?;

        Ok(Self {
            name: unit.to_string(),
            status,
            freezer,
        })
    }
}

#[derive(Default, Serialize)]
struct Status {
    promoter: Vec<PromoterStatus>,
    prometheus: Vec<PrometheusStatus>,
    debugger: Vec<DebuggerStatus>,
    umh: Vec<UMHStatus>,
    agentx: Vec<AgentXStatus>,
}

impl Status {
    fn format(&self, fmt: &Format, verbose: bool) -> Result<String> {
        match fmt {
            Format::Terminal => self.terminal(verbose),
            Format::Json => self.json(),
        }
    }
    fn terminal(&self, verbose: bool) -> Result<String> {
        let mut w = String::new();

        for p in &self.promoter {
            write!(w, "{}", p.terminal(verbose)?)?;
        }
        for p in &self.prometheus {
            write!(w, "{}", p.terminal(verbose)?)?;
        }
        for p in &self.debugger {
            write!(w, "{}", p.terminal(verbose)?)?;
        }
        for p in &self.umh {
            write!(w, "{}", p.terminal(verbose)?)?;
        }
        for p in &self.agentx {
            write!(w, "{}", p.terminal(verbose)?)?;
        }

        Ok(w)
    }
    fn json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

#[derive(Serialize)]
struct PromoterStatus {
    drbd_resource: String,
    path: PathBuf,
    enabled: bool,
    primary_on: PrimaryOn,
    target: SystemdUnit,
    dependencies: Vec<SystemdUnit>,
    status: UnitActiveState,
}

impl PromoterStatus {
    fn terminal(&self, verbose: bool) -> Result<String> {
        let mut w = String::new();
        writeln!(w, "{}:", self.path.display())?;
        let state = enabled_str(self.enabled);
        writeln!(
            w,
            "Promoter: Resource {}{} currently active on {}",
            self.drbd_resource,
            state,
            self.primary_on.terminal(verbose)?
        )?;

        if !self.enabled {
            if let PrimaryOn::Local(_) = &self.primary_on {
                writeln!(w, "{}", warn_str("disabled but Primary"))?;
            }
            return Ok(w);
        }

        writeln!(
            w,
            "{} {}",
            self.target.status.terminal(verbose)?,
            self.target.name
        )?;

        for (i, unit) in self.dependencies.iter().enumerate() {
            let sep = if i == self.dependencies.len() - 1 {
                "└─"
            } else {
                "├─"
            };
            write!(
                w,
                "{} {} {}",
                unit.status.terminal(verbose)?,
                sep,
                unit.name
            )?;
            match unit.freezer {
                UnitFreezerState::Running | UnitFreezerState::Unknown => writeln!(w)?,
                _ => writeln!(w, "({})", unit.freezer.terminal(verbose)?)?,
            };
        }

        Ok(w)
    }
}

#[derive(Serialize)]
struct PrometheusStatus {
    path: PathBuf,
    enabled: bool,
    address: config::LocalAddress,
    status: UnitActiveState,
}
impl PrometheusStatus {
    fn terminal(&self, verbose: bool) -> Result<String> {
        let mut w = String::new();
        writeln!(
            w,
            "Prometheus: listening on {}",
            self.address.to_string().bold().green()
        )?;

        if verbose {
            for addr in self.address.to_socket_addrs()? {
                let status = match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
                    Ok(_) => format!("{}", "success".bold().green()),
                    Err(e) => format!("{} ({})", "failed".bold().red(), e),
                };
                writeln!(w, "TCP Connect ({}): {}", addr, status)?;
            }
        }

        Ok(w)
    }
}
#[derive(Serialize)]
struct DebuggerStatus {
    path: PathBuf,
    enabled: bool,
    status: UnitActiveState,
}
impl DebuggerStatus {
    fn terminal(&self, _verbose: bool) -> Result<String> {
        let mut w = String::new();
        writeln!(w, "Debugger: {}", "started".bold().green())?;
        Ok(w)
    }
}

#[derive(Serialize)]
struct UMHStatus {
    path: PathBuf,
    enabled: bool,
    status: UnitActiveState,
}
impl UMHStatus {
    fn terminal(&self, _verbose: bool) -> Result<String> {
        let mut w = String::new();
        writeln!(w, "UMH: {}", "started".bold().green())?;
        Ok(w)
    }
}

#[derive(Serialize)]
struct AgentXStatus {
    path: PathBuf,
    enabled: bool,
    address: String,
    status: UnitActiveState,
}
impl AgentXStatus {
    fn terminal(&self, _verbose: bool) -> Result<String> {
        let mut w = String::new();
        writeln!(
            w,
            "AgentX: connecting to main agent at {}",
            self.address.bold().green()
        )?;
        Ok(w)
    }
}

fn green_str(text: &str) -> String {
    format!("{}", text.bold().green())
}

fn green(text: &str) {
    println!("{}", green_str(text))
}

fn warn_str(text: &str) -> String {
    format!("{} {}", "WARN:".bold().red(), text)
}

fn warn(text: &str) {
    println!("{}", warn_str(text))
}

fn info_str(text: &str) -> String {
    format!("{} {}", "INFO:".bold().yellow(), text)
}

fn info(text: &str) {
    println!("{}", info_str(text))
}

const PROMOTER_TEMPLATE: &str = r###"[[promoter]]
[promoter.resources.$resname]
start = ["$service.mount", "$service.service"]
# on-drbd-demote-failure = "reboot"
# stop-services-on-exit = false
# on-disk-detach = "ignore"
#
# for more complex setups like HA iSCSI targets, NFS exports, or NVMe-oF targets consider
# https://github.com/LINBIT/linstor-gateway which uses LINSTOR and drbd-reactor"###;

const PROMETHEUS_TEMPLATE: &str = r###"[[prometheus]]
enums = true
# address = ":9942""###;

const AGENTX_TEMPLATE: &str = r###"[[agentx]]
## adress of the main SNMP daemon AgentX TCP socket
# address = "localhost:705"
# cache-max = 60 # seconds
# agent-timeout = 60 # seconds snmpd waits for an answer
# peer-states = true # include peer connection and disk states"###;

const UMH_TEMPLATE: &str = r###"[[umh]]
[[umh.resource]]
command = "slack.sh $DRBD_RES_NAME on $(uname -n) from $DRBD_OLD_ROLE to $DRBD_NEW_ROLE"
event-type = "Change"
old.role = { operator = "NotEquals", value = "Primary" }
new.role = "Primary"
# This is a trivial resource rule example, please see drbd-reactor.umh(5) for more examples"###;

const DEBUGGER_TEMPLATE: &str = r###"[[debugger]]
# NOTE: make sure the log level in your [[log]] section is at least on level 'debug'"###;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_snippets_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("foo.toml"), "[[promoter]]").unwrap();
        fs::write(dir.path().join("bar.toml.disabled"), "[[promoter]]").unwrap();
        fs::write(dir.path().join("baz.toml"), "[[promoter]]").unwrap();
        dir
    }

    // --- snippet_state_of ---

    #[test]
    fn snippet_state_of_toml() {
        assert_eq!(
            snippet_state_of(Path::new("/etc/drbd-reactor.d/foo.toml")),
            Some(SnippetState::Enabled)
        );
    }

    #[test]
    fn snippet_state_of_toml_disabled() {
        assert_eq!(
            snippet_state_of(Path::new("/etc/drbd-reactor.d/foo.toml.disabled")),
            Some(SnippetState::Disabled)
        );
    }

    #[test]
    fn snippet_state_of_no_extension() {
        assert_eq!(snippet_state_of(Path::new("foo")), None);
    }

    #[test]
    fn snippet_state_of_wrong_extension() {
        assert_eq!(snippet_state_of(Path::new("foo.txt")), None);
    }

    #[test]
    fn snippet_state_of_dotted_stem() {
        assert_eq!(
            snippet_state_of(Path::new("my.service.toml")),
            Some(SnippetState::Enabled)
        );
        assert_eq!(
            snippet_state_of(Path::new("my.service.toml.disabled")),
            Some(SnippetState::Disabled)
        );
    }

    // --- with_snippet_extension ---

    #[test]
    fn with_snippet_extension_bare_to_enabled() {
        assert_eq!(
            with_snippet_extension(Path::new("foo"), SnippetState::Enabled),
            PathBuf::from("foo.toml")
        );
    }

    #[test]
    fn with_snippet_extension_bare_to_disabled() {
        assert_eq!(
            with_snippet_extension(Path::new("foo"), SnippetState::Disabled),
            PathBuf::from("foo.toml.disabled")
        );
    }

    #[test]
    fn with_snippet_extension_enabled_to_disabled() {
        assert_eq!(
            with_snippet_extension(Path::new("foo.toml"), SnippetState::Disabled),
            PathBuf::from("foo.toml.disabled")
        );
    }

    #[test]
    fn with_snippet_extension_disabled_to_enabled() {
        assert_eq!(
            with_snippet_extension(Path::new("foo.toml.disabled"), SnippetState::Enabled),
            PathBuf::from("foo.toml")
        );
    }

    #[test]
    fn with_snippet_extension_dotted_stem() {
        assert_eq!(
            with_snippet_extension(Path::new("my.service.toml"), SnippetState::Disabled),
            PathBuf::from("my.service.toml.disabled")
        );
        assert_eq!(
            with_snippet_extension(Path::new("my.service.toml.disabled"), SnippetState::Enabled),
            PathBuf::from("my.service.toml")
        );
    }

    // --- opposite_snippet_path ---

    #[test]
    fn opposite_of_toml() {
        assert_eq!(
            opposite_snippet_path(Path::new("/etc/foo.toml")),
            Some(PathBuf::from("/etc/foo.toml.disabled"))
        );
    }

    #[test]
    fn opposite_of_toml_disabled() {
        assert_eq!(
            opposite_snippet_path(Path::new("/etc/foo.toml.disabled")),
            Some(PathBuf::from("/etc/foo.toml"))
        );
    }

    #[test]
    fn opposite_of_no_extension() {
        assert_eq!(opposite_snippet_path(Path::new("foo")), None);
    }

    // --- resolve_snippets ---

    #[test]
    fn resolve_bare_name_enabled() {
        let dir = setup_snippets_dir();
        let result = resolve_snippets(dir.path(), Some(&["foo".into()]), SnippetState::Enabled);
        assert_eq!(result, vec![dir.path().join("foo.toml")]);
    }

    #[test]
    fn resolve_bare_name_disabled() {
        let dir = setup_snippets_dir();
        let result = resolve_snippets(dir.path(), Some(&["foo".into()]), SnippetState::Disabled);
        assert_eq!(result, vec![dir.path().join("foo.toml.disabled")]);
    }

    #[test]
    fn resolve_name_with_toml_extension() {
        let dir = setup_snippets_dir();
        let result = resolve_snippets(
            dir.path(),
            Some(&["foo.toml".into()]),
            SnippetState::Enabled,
        );
        assert_eq!(result, vec![dir.path().join("foo.toml")]);
    }

    #[test]
    fn resolve_name_with_toml_disabled_extension() {
        // This is the bug fix: .toml.disabled must be accepted
        let dir = setup_snippets_dir();
        let result = resolve_snippets(
            dir.path(),
            Some(&["bar.toml.disabled".into()]),
            SnippetState::Disabled,
        );
        assert_eq!(result, vec![dir.path().join("bar.toml.disabled")]);
    }

    #[test]
    fn resolve_absolute_path_passthrough() {
        let dir = setup_snippets_dir();
        let abs = dir.path().join("foo.toml");
        let result = resolve_snippets(
            dir.path(),
            Some(&[abs.to_str().unwrap().into()]),
            SnippetState::Enabled,
        );
        assert_eq!(result, vec![abs]);
    }

    #[test]
    fn resolve_no_configs_globs_enabled() {
        let dir = setup_snippets_dir();
        let result = resolve_snippets(dir.path(), None, SnippetState::Enabled);
        assert_eq!(
            result,
            vec![dir.path().join("baz.toml"), dir.path().join("foo.toml")]
        );
    }

    #[test]
    fn resolve_no_configs_globs_disabled() {
        let dir = setup_snippets_dir();
        let result = resolve_snippets(dir.path(), None, SnippetState::Disabled);
        assert_eq!(result, vec![dir.path().join("bar.toml.disabled")]);
    }

    #[test]
    fn resolve_multiple_configs() {
        let dir = setup_snippets_dir();
        let result = resolve_snippets(
            dir.path(),
            Some(&["foo".into(), "baz.toml".into()]),
            SnippetState::Enabled,
        );
        assert_eq!(
            result,
            vec![dir.path().join("foo.toml"), dir.path().join("baz.toml")]
        );
    }

    // --- validate_snippets ---

    #[test]
    fn validate_rejects_wrong_extension() {
        let dir = setup_snippets_dir();
        fs::write(dir.path().join("foo.txt"), "").unwrap();
        let paths = vec![dir.path().join("foo.txt")];
        let result = validate_snippets(dir.path(), &paths, false);
        assert!(result.is_empty());
    }

    #[test]
    fn validate_rejects_outside_snippets_path() {
        let dir = setup_snippets_dir();
        let paths = vec![PathBuf::from("/tmp/foo.toml")];
        let result = validate_snippets(dir.path(), &paths, false);
        assert!(result.is_empty());
    }

    #[test]
    fn validate_rejects_nonexistent_when_required() {
        let dir = setup_snippets_dir();
        let paths = vec![dir.path().join("nonexistent.toml")];
        let result = validate_snippets(dir.path(), &paths, true);
        assert!(result.is_empty());
    }

    #[test]
    fn validate_allows_nonexistent_when_not_required() {
        let dir = setup_snippets_dir();
        let paths = vec![dir.path().join("newfile.toml")];
        let result = validate_snippets(dir.path(), &paths, false);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn validate_accepts_existing_file() {
        let dir = setup_snippets_dir();
        let paths = vec![dir.path().join("foo.toml")];
        let result = validate_snippets(dir.path(), &paths, true);
        assert_eq!(result, paths);
    }

    #[test]
    fn validate_warns_when_opposite_exists() {
        let dir = setup_snippets_dir();
        // foo.toml already exists, create its opposite too
        fs::write(dir.path().join("foo.toml.disabled"), "").unwrap();
        let paths = vec![dir.path().join("foo.toml")];
        let result = validate_snippets(dir.path(), &paths, true);
        // Still returned (warning, not rejection)
        assert_eq!(result.len(), 1);
    }

    // --- get_disabled_path / get_enabled_path ---

    #[test]
    fn disabled_path_from_toml() {
        assert_eq!(
            get_disabled_path(Path::new("/etc/drbd-reactor.d/foo.toml")),
            PathBuf::from("/etc/drbd-reactor.d/foo.toml.disabled")
        );
    }

    #[test]
    fn enabled_path_from_disabled() {
        assert_eq!(
            get_enabled_path(Path::new("/etc/drbd-reactor.d/foo.toml.disabled")).unwrap(),
            PathBuf::from("/etc/drbd-reactor.d/foo.toml")
        );
    }

    #[test]
    fn disabled_path_dotted_stem() {
        assert_eq!(
            get_disabled_path(Path::new("/etc/drbd-reactor.d/my.service.toml")),
            PathBuf::from("/etc/drbd-reactor.d/my.service.toml.disabled")
        );
    }

    #[test]
    fn enabled_path_dotted_stem() {
        assert_eq!(
            get_enabled_path(Path::new("/etc/drbd-reactor.d/my.service.toml.disabled")).unwrap(),
            PathBuf::from("/etc/drbd-reactor.d/my.service.toml")
        );
    }

    #[test]
    fn enabled_path_rejects_already_enabled() {
        assert!(get_enabled_path(Path::new("/etc/drbd-reactor.d/foo.toml")).is_err());
    }

    #[test]
    fn enabled_path_rejects_no_extension() {
        assert!(get_enabled_path(Path::new("/etc/drbd-reactor.d/foo")).is_err());
    }
}
