use regex::Regex;
use std::collections::HashMap;
use std::ffi::CStr;
use std::fmt;
use std::fs;
use std::fs::File;
use std::io;
use std::io::Write;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use libc::c_char;
use log::{debug, info, trace, warn};
use serde::{Deserialize, Serialize};
use shell_words;
use tinytemplate::TinyTemplate;

use crate::drbd::{DiskState, EventType, PluginUpdate, Resource, Role};
use crate::plugin;

pub struct Promoter {
    cfg: PromoterConfig,
}

impl Promoter {
    pub fn new(cfg: PromoterConfig) -> Result<Self> {
        let names = cfg.resources.keys().cloned().collect::<Vec<String>>();
        if let Err(e) = adjust_resources(&names) {
            warn!("Could not adjust '{:?}': {}", names, e);
        }
        info!("Checking DRBD options for '{:?}'", names);
        if let Err(e) = check_resources(&names) {
            warn!("Could not execute DRBD options check: {}", e);
        }

        for (name, res) in &cfg.resources {
            // deprecated settings
            if !res.on_stop_failure.is_empty() {
                warn!("'on-stop-failure' is deprecated and ignored!; use 'on-drbd-demote-failure'");
            }

            if res.runner == Runner::Systemd {
                let systemd_settings = SystemdSettings {
                    dependencies_as: res.dependencies_as.clone(),
                    target_as: res.target_as.clone(),
                    failure_action: res.on_drbd_demote_failure.clone(),
                };
                generate_systemd_templates(name, &res.start, &systemd_settings)?;
                systemd_daemon_reload()?;
            }
        }

        Ok(Self { cfg })
    }
}

impl super::Plugin for Promoter {
    fn run(&self, rx: super::PluginReceiver) -> Result<()> {
        trace!("run: start");

        let type_exists = plugin::typefilter(&EventType::Exists);
        let type_change = plugin::typefilter(&EventType::Change);
        let names = self.cfg.resources.keys().cloned().collect::<Vec<String>>();
        let names = plugin::namefilter(&names);

        // set default stop actions (i.e., reversed start)
        let cfg = {
            let mut cfg = self.cfg.clone();
            for res in cfg.resources.values_mut() {
                if res.stop.is_empty() {
                    res.stop = res.start.clone();
                    res.stop.reverse();
                }
            }
            cfg
        };

        for r in rx
            .into_iter()
            .filter(names)
            .filter(|x| type_exists(x) || type_change(x))
        {
            let name = r.get_name();
            let res = cfg
                .resources
                .get(&name)
                .expect("Can not happen, name filter is built from the cfg");

            match r.as_ref() {
                PluginUpdate::Resource(u) => {
                    if !u.old.may_promote && u.new.may_promote {
                        let sleep_millis = get_sleep_before_promote_ms(
                            &u.resource,
                            &res.preferred_nodes,
                            res.sleep_before_promote_factor,
                        );
                        info!(
                            "run: resource '{}' may promote after {}ms",
                            name, sleep_millis
                        );
                        if sleep_millis > 0 {
                            thread::sleep(Duration::from_millis(sleep_millis));
                        }
                        if start_actions(&name, &res.start, &res.runner).is_err() {
                            if let Err(e) = stop_actions(&name, &res.stop, &res.runner) {
                                warn!("Stopping '{}' failed: {}", name, e);
                            }
                        }
                    }
                }
                PluginUpdate::Device(u) => {
                    if u.old.quorum && !u.new.quorum {
                        info!("run: resource '{}' lost quorum", name);
                        if let Err(e) = stop_actions(&name, &res.stop, &res.runner) {
                            warn!("Stopping '{}' failed: {}", name, e);
                        }
                    }
                }
                PluginUpdate::PeerDevice(u) => {
                    if res.preferred_nodes.len() == 0 {
                        continue;
                    } else if !(u.old.peer_disk_state != DiskState::UpToDate
                        && u.new.peer_disk_state == DiskState::UpToDate)
                    {
                        continue;
                    } else if u.resource.role != Role::Primary {
                        continue;
                    }

                    let peer_name = match u.resource.get_peerdevice(u.peer_node_id, u.volume) {
                        Some(pd) => pd.conn_name.clone(),
                        None => {
                            warn!("Could not find peer device for resource '{}'", name);
                            continue;
                        }
                    };
                    let peer_pos = match res.preferred_nodes.iter().position(|n| n == &peer_name) {
                        Some(pos) => pos,
                        None => {
                            // not in the list, it can not be better
                            debug!(
                                "Peer '{}' was not found in preferred_nodes, continue",
                                peer_name
                            );
                            continue;
                        }
                    };

                    let node_name = match uname_n() {
                        Ok(node_name) => node_name,
                        Err(e) => {
                            warn!("Could not determine 'uname -n': {}", e);
                            continue;
                        }
                    };
                    let node_pos = match res.preferred_nodes.iter().position(|n| n == &node_name) {
                        Some(pos) => pos,
                        None => res.preferred_nodes.len(),
                    };

                    if peer_pos < node_pos {
                        info!(
                            "run: resource '{}' has a new preferred node ('{}'), stopping services locally ('{}')",
                            name, peer_name, node_name
                        );
                        if let Err(e) = stop_actions(&name, &res.stop, &res.runner) {
                            warn!("Stopping '{}' failed: {}", name, e);
                        }
                    }
                }
                _ => (),
            }
        }

        // stop services if configured
        for (name, res) in cfg.resources {
            if res.stop_services_on_exit {
                let shutdown = || -> Result<()> {
                    fs::remove_file(escaped_services_target_dir(&name).join(SYSTEMD_BEFORE_CONF))?;
                    systemd_daemon_reload()?;
                    stop_actions(&name, &res.stop, &res.runner)
                };
                if let Err(e) = shutdown() {
                    warn!("Stopping '{}' failed: {}", name, e);
                }
            }
        }

        trace!("run: exit");
        Ok(())
    }

    fn get_id(&self) -> Option<String> {
        self.cfg.id.clone()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct PromoterConfig {
    #[serde(default)]
    pub resources: HashMap<String, PromoterOptResource>,
    pub id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct PromoterOptResource {
    #[serde(default)]
    pub start: Vec<String>,
    #[serde(default)]
    pub stop: Vec<String>,
    #[serde(default)]
    pub on_stop_failure: String, // ! deprecated !
    #[serde(default)]
    pub stop_services_on_exit: bool,
    #[serde(default)]
    pub runner: Runner,
    #[serde(default)]
    pub dependencies_as: SystemdDependency,
    #[serde(default)]
    pub target_as: SystemdDependency,
    #[serde(default)]
    pub on_drbd_demote_failure: SystemdFailureAction,
    #[serde(default = "default_promote_sleep")]
    pub sleep_before_promote_factor: f32,
    #[serde(default)]
    pub preferred_nodes: Vec<String>,
}

fn default_promote_sleep() -> f32 {
    1.0
}

fn systemd_stop(unit: &str) -> Result<()> {
    info!("systemd_stop: systemctl stop {}", unit);
    plugin::map_status(Command::new("systemctl").arg("stop").arg(unit).status())
}

fn systemd_start(unit: &str) -> Result<()> {
    // we really don't care
    let _ = Command::new("systemctl")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg("reset-failed")
        .arg(unit)
        .status();

    info!("systemd_start: systemctl start {}", unit);
    plugin::map_status(Command::new("systemctl").arg("start").arg(unit).status())
}

fn persist_journal() {
    let _ = Command::new("journalctl")
        .arg("--flush")
        .arg("--sync")
        .status();
}

fn action(what: &str, to: State, how: &Runner) -> Result<()> {
    match how {
        Runner::Shell => plugin::system(what),
        Runner::Systemd => match to {
            State::Start => systemd_start(what),
            State::Stop => systemd_stop(what),
        },
    }
}

fn start_actions(name: &str, actions: &[String], how: &Runner) -> Result<()> {
    match how {
        Runner::Shell => {
            for a in actions {
                action(a, State::Start, how)?;
            }
            Ok(())
        }
        Runner::Systemd => action(
            &format!("drbd-services@{}.target", escape_name(name)),
            State::Start,
            how,
        ),
    }
}

fn stop_actions(name: &str, actions: &[String], how: &Runner) -> Result<()> {
    info!(
        "stop_actions (could trigger failure actions (e.g., reboot)): {}",
        name
    );

    match how {
        Runner::Shell => {
            for a in actions {
                action(a, State::Stop, how)?;
            }
            Ok(())
        }
        Runner::Systemd => {
            let target = format!("drbd-services@{}.target", escape_name(name));
            info!("stop_actions: stopping '{}'", target);
            persist_journal();
            action(&target, State::Stop, how)
        }
    }
}

fn get_backing_devices(resname: &str) -> Result<Vec<String>> {
    let shlldev = Command::new("drbdadm")
        .arg("sh-ll-dev")
        .arg(resname)
        .output()?;
    if !shlldev.status.success() {
        return Err(anyhow::anyhow!(
            "'drbdadm sh-ll-dev {}' not executed successfully, stdout: '{}', stderr: '{}'",
            resname,
            String::from_utf8(shlldev.stdout).unwrap_or("<Could not convert stdout>".to_string()),
            String::from_utf8(shlldev.stderr).unwrap_or("<Could not convert stderr>".to_string())
        ));
    }

    let shlldev = String::from_utf8(shlldev.stdout)?;
    let devices: Vec<String> = shlldev.lines().map(|s| s.to_string()).collect();
    Ok(devices)
}

fn adjust_resources(to_start: &[String]) -> Result<()> {
    for res in to_start {
        for dev in get_backing_devices(res)? {
            info!(
                "adjust_resources: waiting for backing device '{}' to become ready",
                dev
            );
            while !drbd_backing_device_ready(&dev) {
                thread::sleep(Duration::from_secs(2));
            }
            info!("adjust_resources: backing device '{}' now ready", dev);
        }

        plugin::map_status(Command::new("drbdadm").arg("adjust").arg(res).status())?;
    }
    Ok(())
}

fn drbd_backing_device_ready(dev: &str) -> bool {
    dev == "none"
        || match fs::metadata(dev) {
            Err(_) => false,
            Ok(meta) => meta.file_type().is_block_device(),
        }
}

const SYSTEMD_PREFIX: &str = "/run/systemd/system";
const SYSTEMD_CONF: &str = "reactor.conf";
const SYSTEMD_BEFORE_CONF: &str = "reactor-50-before.conf";

fn generate_systemd_templates(
    name: &str,
    actions: &[String],
    systemd_settings: &SystemdSettings,
) -> Result<()> {
    let escaped_name = escape_name(name);
    let prefix = Path::new(SYSTEMD_PREFIX).join(format!("drbd-promote@{}.service.d", escaped_name));
    systemd_write_unit(prefix, SYSTEMD_CONF, drbd_promote(systemd_settings)?)?;

    if systemd_settings.failure_action != SystemdFailureAction::None {
        let prefix = Path::new(SYSTEMD_PREFIX).join(format!(
            "drbd-demote-or-escalate@{}.service.d",
            escaped_name
        ));
        systemd_write_unit(
            prefix,
            SYSTEMD_CONF,
            format!(
                "[Unit]\nFailureAction={}\nConflicts=drbd-promote@%i.service\n",
                systemd_settings.failure_action
            ),
        )?;
    }

    let mut target_requires: Vec<String> = Vec::new();

    let ocf_pattern = Regex::new(r"^ocf:(\S+):(\S+) (.*)$")?;

    for action in actions {
        let deps = match target_requires.last() {
            Some(prev) => vec![
                format!("drbd-promote@{}.service", escaped_name),
                prev.to_string(),
            ],
            None => vec![format!("drbd-promote@{}.service", escaped_name)],
        };

        let (service_name, env) = match ocf_pattern.captures(action) {
            Some(ocf) => {
                let (vendor, agent, args) = (&ocf[1], &ocf[2], &ocf[3]);
                escaped_systemd_ocf_parse_to_env(name, vendor, agent, args)?
            }
            _ => (action.to_string(), Vec::new()),
        };

        let prefix = Path::new(SYSTEMD_PREFIX).join(format!("{}.d", service_name));
        if service_name.ends_with(".mount") {
            systemd_write_unit(
                prefix.clone(),
                "reactor-50-mount.conf",
                format!("[Unit]\nDefaultDependencies=no\n"),
            )?;
        }
        systemd_write_unit(
            prefix,
            SYSTEMD_CONF,
            systemd_unit(&escaped_name, &deps, systemd_settings, &env)?,
        )?;

        // we would not need to keep the order here, as it does not matter
        // what matters is After=, but IMO it would confuse unexperienced users
        // just keep the order, so no HashSet, the Vecs are short, does not matter.
        if target_requires.contains(&service_name) {
            return Err(anyhow::anyhow!(
                "generate_systemd_templates: Service name '{}' already used",
                service_name
            ));
        }
        target_requires.push(service_name.clone());
    }

    // target and the extra Before= override
    systemd_write_unit(
        escaped_services_target_dir(name),
        SYSTEMD_CONF,
        systemd_target_requires(&target_requires, systemd_settings)?,
    )?;
    systemd_write_unit(
        escaped_services_target_dir(name),
        SYSTEMD_BEFORE_CONF,
        format!("[Unit]\nBefore=drbd-reactor.service\n"),
    )
}

fn escaped_systemd_ocf_parse_to_env(
    name: &str,
    vendor: &str,
    agent: &str,
    args: &str,
) -> Result<(String, Vec<String>)> {
    let args = shell_words::split(args)?;

    if args.len() < 1 {
        anyhow::bail!("promoter::systemd_ocf: agent needs at least one argument (its name)")
    }

    let ra_name = &args[0];
    let ra_name = format!("{}_{}", ra_name, name);
    let escaped_ra_name = escape_name(&ra_name);
    let service_name = format!("ocf.ra@{}.service", escaped_ra_name);
    let mut env = Vec::with_capacity(args.len() - 1);
    for item in &args[1..] {
        let mut split = item.splitn(2, "=");
        let add = match (split.next(), split.next()) {
            (Some(k), Some(v)) => format!("OCF_RESKEY_{}={}", k, shell_words::quote(v)),
            (Some(k), None) => format!("OCF_RESKEY_{}=", k),
            _ => continue, // skip empty items
        };
        env.push(add)
    }

    env.push(format!(
        "AGENT=/usr/lib/ocf/resource.d/{}/{}",
        vendor, agent
    ));

    Ok((service_name, env))
}

fn drbd_promote(systemd_settings: &SystemdSettings) -> Result<String> {
    const PROMOTE_TEMPLATE: &str = r"[Service]
ExecStart=/lib/drbd/scripts/drbd-service-shim.sh primary %I
ExecCondition=
[Unit]
{{ if needs_on_failure -}}
OnFailure=drbd-demote-or-escalate@%i.service
OnFailureJobMode=replace-irreversibly
{{ endif -}}";

    let mut tt = TinyTemplate::new();
    tt.add_template("devices", PROMOTE_TEMPLATE)?;

    #[derive(Serialize)]
    struct Context {
        strictness: String,
        needs_on_failure: bool,
    }
    // filter diskless (== "none" devices)
    let result = tt.render(
        "devices",
        &Context {
            strictness: systemd_settings.dependencies_as.to_string(),
            needs_on_failure: systemd_settings.failure_action != SystemdFailureAction::None,
        },
    )?;
    Ok(result)
}

// does not do further escaping, caller needs to do it
fn systemd_unit(
    name: &str,
    deps: &[String],
    systemd_settings: &SystemdSettings,
    env: &[String],
) -> Result<String> {
    const UNIT_TEMPLATE: &str = r"[Unit]
Description=drbd-reactor controlled %p
PartOf = drbd-services@{name}.target
{{ for dep in deps }}
{strictness} = {dep | unescaped}
After = {dep}
{{- endfor -}}

{{ for e in env }}
{{ if @first  }}
[Service]
{{ endif -}}
Environment= {e | unescaped}
{{- endfor -}}";

    let mut tt = TinyTemplate::new();
    tt.add_template("unit", UNIT_TEMPLATE)?;

    #[derive(Serialize)]
    struct Context<'a> {
        name: String,
        deps: &'a [String],
        env: &'a [String],
        strictness: String,
    }
    let result = tt.render(
        "unit",
        &Context {
            name: name.to_string(),
            deps,
            env,
            strictness: systemd_settings.dependencies_as.to_string(),
        },
    )?;
    Ok(result)
}

fn systemd_target_requires(
    requires: &[String],
    systemd_settings: &SystemdSettings,
) -> Result<String> {
    const WANTS_TEMPLATE: &str = r"[Unit]
{{- for require in requires }}
{strictness} = {require | unescaped}
{{- endfor -}}";

    let mut tt = TinyTemplate::new();
    tt.add_template("requires", WANTS_TEMPLATE)?;

    #[derive(Serialize)]
    struct Context<'a> {
        requires: &'a [String],
        strictness: String,
    }
    let result = tt.render(
        "requires",
        &Context {
            requires,
            strictness: systemd_settings.target_as.to_string(),
        },
    )?;
    Ok(result)
}

fn systemd_write_unit(prefix: PathBuf, unit: &str, content: String) -> Result<()> {
    let content = format!("# Auto-generated by drbd-reactor, DO NOT EDIT\n{}", content);
    let path = prefix.join(unit);
    let tmp_path = prefix.join(format!("{}.tmp", unit));
    info!("systemd_write_unit: creating {:?}", path);

    fs::create_dir_all(&prefix)?;
    {
        let mut f = File::create(&tmp_path)?;
        f.write_all(content.as_bytes())?;
        f.write_all("\n".as_bytes())?;
    }
    fs::rename(tmp_path, path)?;

    Ok(())
}

fn systemd_daemon_reload() -> Result<()> {
    info!("systemd_daemon_reload: reloading daemon");
    plugin::system("systemctl daemon-reload")
}

enum State {
    Start,
    Stop,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum SystemdDependency {
    Wants,
    Requires,
    Requisite,
    BindsTo,
}
impl Default for SystemdDependency {
    fn default() -> Self {
        Self::Requires
    }
}
impl fmt::Display for SystemdDependency {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Wants => write!(f, "Wants"),
            Self::Requires => write!(f, "Requires"),
            Self::Requisite => write!(f, "Requisite"),
            Self::BindsTo => write!(f, "BindsTo"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum SystemdFailureAction {
    None,
    Reboot,
    RebootForce,
    RebootImmediate,
    Poweroff,
    PoweroffForce,
    PoweroffImmediate,
    Exit,
    ExitForce,
}
impl Default for SystemdFailureAction {
    fn default() -> Self {
        Self::None
    }
}
impl fmt::Display for SystemdFailureAction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Reboot => write!(f, "reboot"),
            Self::RebootForce => write!(f, "reboot-force"),
            Self::RebootImmediate => write!(f, "reboot-immediate"),
            Self::Poweroff => write!(f, "poweroff"),
            Self::PoweroffForce => write!(f, "poweroff-force"),
            Self::PoweroffImmediate => write!(f, "poweroff-immediate"),
            Self::Exit => write!(f, "exit"),
            Self::ExitForce => write!(f, "exit-force"),
        }
    }
}

struct SystemdSettings {
    dependencies_as: SystemdDependency,
    target_as: SystemdDependency,
    failure_action: SystemdFailureAction,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum Runner {
    #[serde(rename = "systemd")]
    Systemd,
    #[serde(rename = "shell")]
    Shell,
}
impl Default for Runner {
    fn default() -> Self {
        Self::Systemd
    }
}

fn get_sleep_before_promote_ms(
    resource: &Resource,
    preferred_nodes: &[String],
    factor: f32,
) -> u64 {
    let mut max_sleep_s: usize = resource
        .devices
        .iter()
        .map(|d| match d.disk_state {
            DiskState::Diskless => 6,
            DiskState::Attaching => 6,
            DiskState::Detaching => 6,
            DiskState::Failed => 6,
            DiskState::Negotiating => 6,
            DiskState::DUnknown => 6,
            DiskState::Inconsistent => 3,
            DiskState::Outdated => 2,
            DiskState::Consistent => 1,
            DiskState::UpToDate => 0,
        })
        .max() // if there are none, try the res file
        .unwrap_or_else(|| match get_backing_devices(&resource.name) {
            Ok(devices) if devices.contains(&"none".into()) => 6, // Diskless
            _ => 0,
        });

    match uname_n() {
        Ok(node_name) => {
            max_sleep_s += match preferred_nodes.iter().position(|n| n == &node_name) {
                Some(pos) => pos,
                None => preferred_nodes.len(),
            };
        }
        Err(e) => warn!("Could not determine 'uname -n': {}", e),
    };

    // convert to ms and scale by factor
    (max_sleep_s as f32 * 1000.0 * factor).ceil() as u64
}

fn escaped_services_target_dir(name: &str) -> PathBuf {
    Path::new(SYSTEMD_PREFIX).join(format!("drbd-services@{}.target.d", escape_name(name)))
}

fn check_resources(to_start: &[String]) -> Result<()> {
    #[derive(Serialize, Deserialize)]
    struct Resource {
        resource: String,
        options: Options,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    struct Options {
        auto_promote: bool,
        quorum: String,
        on_no_quorum: String,
    }

    let check_for = |res: &str, what: &str, expected: &str, is: &str| {
        if expected != is {
            warn!(
                "resource '{}': DRBD option '{}' should be '{}', but is '{}'",
                res, what, expected, is
            );
        }
    };

    for res in to_start {
        let output = Command::new("drbdsetup")
            .arg("show")
            .arg("--show-defaults")
            .arg("--json")
            .arg(res)
            .output()?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "'drbdsetup show' not executed successfully"
            ));
        }

        let stdout = String::from_utf8(output.stdout)?;
        let resources: Vec<Resource> = serde_json::from_str(&stdout)?;
        if resources.len() != 1 {
            return Err(anyhow::anyhow!(
                "resources lenght from drbdsetup show not exactly 1"
            ));
        }
        if &resources[0].resource != res {
            return Err(anyhow::anyhow!(
                "res name to check ('{}') and drbdsetup show output ('{}') did not match",
                res,
                resources[0].resource
            ));
        }

        check_for(
            res,
            "auto-promote",
            "no",
            match resources[0].options.auto_promote {
                true => "yes",
                false => "no",
            },
        );
        check_for(res, "quorum", "majority", &resources[0].options.quorum);
        check_for(
            res,
            "on-no-quorum",
            "io-error",
            &resources[0].options.on_no_quorum,
        );
    }
    Ok(())
}

// inspired by https://crates.io/crates/uname
// inlined because currently not packaged in Ubuntu Focal
#[inline]
fn to_cstr(buf: &[c_char]) -> &CStr {
    unsafe { CStr::from_ptr(buf.as_ptr()) }
}
fn uname_n() -> Result<String> {
    let mut n = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::uname(&mut n) };
    if r == 0 {
        Ok(to_cstr(&n.nodename[..]).to_string_lossy().into_owned())
    } else {
        Err(anyhow::anyhow!(io::Error::last_os_error()))
    }
}

// inlined copy from https://crates.io/crates/libsystemd
// inlined because currently not packaged in Ubuntu Focal
pub fn escape_name(name: &str) -> String {
    if name.is_empty() {
        return "".to_string();
    }

    let parts: Vec<String> = name
        .bytes()
        .enumerate()
        .map(|(n, b)| escape_byte(b, n))
        .collect();
    parts.join("")
}

// inlined copy from https://crates.io/crates/libsystemd
// inlined because currently not packaged in Ubuntu Focal
fn escape_byte(b: u8, index: usize) -> String {
    let c = char::from(b);
    match c {
        '/' => '-'.to_string(),
        ':' | '_' | '0'..='9' | 'a'..='z' | 'A'..='Z' => c.to_string(),
        '.' if index > 0 => c.to_string(),
        _ => format!(r#"\x{:02x}"#, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drbd::Device;

    #[test]
    fn sleep_before_promote_ms() {
        // be careful to only use a Resource *with* devices filter out the unwarp_or_else?
        let r = Resource {
            name: "test".to_string(),
            devices: vec![
                Device {
                    disk_state: DiskState::Diskless,
                    ..Default::default()
                },
                Device {
                    disk_state: DiskState::Failed,
                    ..Default::default()
                },
                Device {
                    disk_state: DiskState::UpToDate,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(get_sleep_before_promote_ms(&r, &vec![], 1.0), 6000);
        assert_eq!(get_sleep_before_promote_ms(&r, &vec![], 0.5), 3000);
        if let Ok(node_name) = uname_n() {
            assert_eq!(
                get_sleep_before_promote_ms(
                    &r,
                    &vec![
                        "".to_string(),
                        "".to_string(),
                        node_name.clone(),
                        "".to_string()
                    ],
                    1.0
                ),
                6000 + 2000
            );
            assert_eq!(
                get_sleep_before_promote_ms(
                    &r,
                    &vec!["".to_string(), "".to_string(), "".to_string()],
                    1.0
                ),
                6000 + 3000
            );
        }
    }

    #[test]
    fn test_systemd_ocf_parse_to_env() {
        let (name, env) = escaped_systemd_ocf_parse_to_env(
            "res1",
            "vendor1",
            "agent1",
            "name1 k1=v1 k2=\"with whitespace\" k3=with\\ different\\ whitespace foo empty=''",
        )
        .expect("should work");

        assert_eq!(name, "ocf.ra@name1_res1.service");
        assert_eq!(
            &env[..],
            &[
                "OCF_RESKEY_k1=v1",
                "OCF_RESKEY_k2='with whitespace'",
                "OCF_RESKEY_k3='with different whitespace'",
                "OCF_RESKEY_foo=",
                "OCF_RESKEY_empty=''",
                "AGENT=/usr/lib/ocf/resource.d/vendor1/agent1"
            ]
        );

        // escaping
        let (name, _env) =
            escaped_systemd_ocf_parse_to_env("res-1", "vendor1", "agent1", "name-1 do not care")
                .expect("should work");

        assert_eq!(name, "ocf.ra@name\\x2d1_res\\x2d1.service");
    }

    #[test]
    fn test_drbd_promote() {
        let empty = drbd_promote(&SystemdSettings {
            target_as: SystemdDependency::Wants,
            dependencies_as: SystemdDependency::Wants,
            failure_action: SystemdFailureAction::None,
        })
        .expect("should work");

        assert_eq!(
            r"[Service]
ExecStart=/lib/drbd/scripts/drbd-service-shim.sh primary %I
ExecCondition=
[Unit]
",
            empty
        );

        let on_failure = drbd_promote(&SystemdSettings {
            target_as: SystemdDependency::Wants,
            dependencies_as: SystemdDependency::Wants,
            failure_action: SystemdFailureAction::Reboot,
        })
        .expect("should work");

        assert_eq!(
            r"[Service]
ExecStart=/lib/drbd/scripts/drbd-service-shim.sh primary %I
ExecCondition=
[Unit]
OnFailure=drbd-demote-or-escalate@%i.service
OnFailureJobMode=replace-irreversibly
",
            on_failure
        );
    }
}
