use glob::glob;
use std::collections::HashMap;
use std::fs::read_to_string;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use std::{io, sync, thread};

use anyhow::{Context, Result};
use fern;
use log::{debug, error, warn};
use regex::Regex;
use signal_hook::iterator::Signals;
use structopt::StructOpt;

use drbd_reactor::drbd::{EventType, EventUpdate, PluginUpdate, Resource};
use drbd_reactor::events::events2;
use drbd_reactor::{config, plugin};

#[derive(Debug, StructOpt)]
struct CliOpt {
    #[structopt(
        short,
        long,
        parse(from_os_str),
        default_value = "/etc/drbd-reactor.toml"
    )]
    config: PathBuf,
}

fn read_snippets(path: &PathBuf) -> Result<String> {
    if !path.exists() || !path.is_dir() || !path.is_absolute() {
        return Ok("".to_string());
    }

    let mut path = PathBuf::from(path);
    path.push("*.toml");
    let path = path
        .to_str()
        .ok_or(anyhow::anyhow!("Path not a valid string"))?;

    let mut snippets: Vec<PathBuf> = glob(path)?.filter_map(Result::ok).collect();
    snippets.sort();

    let mut s = String::new();
    for snippet in snippets {
        s.push_str(&read_to_string(snippet)?);
        s.push('\n');
    }

    Ok(s)
}

pub fn read_config() -> Result<config::Config> {
    let cli_opt = CliOpt::from_args();

    let mut content = read_to_string(&cli_opt.config)
        .with_context(|| format!("Could not read config file: {}", cli_opt.config.display()))?;

    let mut config: config::Config = toml::from_str(&content).with_context(|| {
        format!(
            "Could not parse main config file; content: {}",
            cli_opt.config.display()
        )
    })?;

    let snippets_path = match config.snippets {
        None => return Ok(config),
        Some(path) => path,
    };

    let snippets =
        read_snippets(&snippets_path).with_context(|| format!("Could not read config snippets"))?;
    content.push_str("# Content from snippets:\n");
    content.push_str(&snippets);
    config = toml::from_str(&content).with_context(|| {
        format!(
            "Could not parse config files including snippets; content: {}",
            content
        )
    })?;

    Ok(config)
}

/// Core handles DRBD events based on the provided configuration
///
/// It will
/// * start a listener thread, which runs "drbdsetup events2" and converts the events to structs
/// * on the main thread, receive structs from the listener, keeping its state-of-the-world in
///   sync.
/// * Based on the struct received and the existing state, the main thread will forward the events
///   to the plugin channels, enhancing the raw event with additional information like:
///   - the old state
///   - the new state
///   - the overall resource state
struct Core {
    resources: HashMap<String, Resource>,
}

enum CoreExit {
    Stop,
    Reload,
}

impl Core {
    /// Initialize a new Core
    ///
    /// The Core is empty (i.e. does not store any state) until it is run.
    fn new() -> Core {
        Core {
            resources: HashMap::new(),
        }
    }

    fn get_or_create_resource(&mut self, name: &str) -> &mut Resource {
        self.resources
            .entry(name.into())
            .or_insert(Resource::with_name(name))
    }

    /// Start the core
    ///
    /// This will start listening for DRBD events, keeping track of any changes, updating the
    /// state of the world and forwarding this information to all plugins.
    fn run(
        &mut self,
        e2rx: &sync::mpsc::Receiver<EventUpdate>,
        started: &Vec<plugin::PluginStarted>,
    ) -> Result<CoreExit> {
        let _send_updates = |up: Option<PluginUpdate>,
                             res: &Resource,
                             et: &EventType,
                             only_new: bool|
         -> Result<()> {
            if let Some(up) = up {
                let up = sync::Arc::new(up);
                for p in started {
                    if !p.new && only_new {
                        continue;
                    }
                    if let plugin::PluginType::Change = p.ptype {
                        p.tx.send(up.clone())?;
                    }
                }
            }
            let up = PluginUpdate::ResourceOnly(et.clone(), res.clone());
            let up = sync::Arc::new(up);
            for p in started {
                if !p.new && only_new {
                    continue;
                }
                if let plugin::PluginType::Event = p.ptype {
                    p.tx.send(up.clone())?;
                }
            }
            Ok(())
        };
        let send_updates = |up: Option<PluginUpdate>,
                            res: &Resource,
                            et: &EventType|
         -> Result<()> { _send_updates(up, res, et, false) };
        let send_updates_only_new = |up: Option<PluginUpdate>,
                                     res: &Resource,
                                     et: &EventType|
         -> Result<()> { _send_updates(up, res, et, true) };

        // initial state, if there is one for new plugins
        for res in self.resources.values() {
            let ups = res.to_plugin_updates();
            for up in ups {
                let r = up.get_resource();
                send_updates_only_new(Some(up), &r, &EventType::Exists)?;
            }
        }

        for r in e2rx {
            match r {
                EventUpdate::Resource(et, r) => {
                    let res = self.get_or_create_resource(&r.name);
                    let up = res.get_resource_update(&et, &r);
                    send_updates(up, &res, &et)?;

                    if et == EventType::Destroy {
                        self.resources.remove(&r.name);
                    }
                }
                EventUpdate::Device(et, d) => {
                    let res = self.get_or_create_resource(&d.name);
                    let up = res.get_device_update(&et, &d);
                    send_updates(up, &res, &EventType::Change)?;
                }
                EventUpdate::PeerDevice(et, pd) => {
                    let res = self.get_or_create_resource(&pd.name);
                    let up = res.get_peerdevice_update(&et, &pd);
                    send_updates(up, &res, &EventType::Change)?;
                }
                EventUpdate::Connection(et, c) => {
                    let res = self.get_or_create_resource(&c.name);
                    let up = res.get_connection_update(&et, &c);
                    send_updates(up, &res, &EventType::Change)?;
                }
                EventUpdate::Path(et, p) => {
                    let res = self.get_or_create_resource(&p.name);
                    let up = res.get_path_update(&et, &p);
                    send_updates(up, &res, &EventType::Change)?;
                }
                EventUpdate::Stop => return Ok(CoreExit::Stop),
                EventUpdate::Reload => return Ok(CoreExit::Reload),
            }
        }

        Ok(CoreExit::Stop)
    }
}

/// Initialize all configured loggers and set them up as global log sink
fn init_loggers(log_cfgs: Vec<config::LogConfig>) -> Result<()> {
    let mut central_dispatcher = fern::Dispatch::new().format(|out, message, record| {
        out.finish(format_args!(
            "{} [{}] {}",
            record.level(),
            record.target(),
            message,
        ))
    });

    for log_cfg in log_cfgs {
        let out: fern::Output = match log_cfg.file {
            Some(path) => fern::log_file(path)?.into(),
            None => io::stderr().into(),
        };

        let dispatch_for_cfg = fern::Dispatch::new().level(log_cfg.level).chain(out);

        central_dispatcher = central_dispatcher.chain(dispatch_for_cfg);
    }

    central_dispatcher
        .apply()
        .context("failed to set up logging")?;

    Ok(())
}

fn main() -> Result<()> {
    if let Err(e) = min_drbd_versions() {
        error!("main: minimum DRBD version not fullfilled: {}", e);
        std::process::exit(1);
    }
    let mut cfg = read_config()?;
    let statistics_poll = Duration::from_secs(cfg.statistics_poll_interval);

    let (e2tx, e2rx) = sync::mpsc::channel();
    let done = e2tx.clone();
    thread::spawn(move || {
        if let Err(e) = events2(e2tx, statistics_poll) {
            error!("main: events2 processing failed: {}", e);
            std::process::exit(1);
        }
    });

    init_loggers(cfg.clone().log)?;
    thread::spawn(move || {
        let mut signals = Signals::new(&[libc::SIGHUP, libc::SIGINT, libc::SIGTERM]).unwrap();
        for signal in signals.forever() {
            debug!("main: sighandler loop");
            match signal as libc::c_int {
                libc::SIGHUP => {
                    done.send(EventUpdate::Reload).unwrap();
                }
                libc::SIGINT | libc::SIGTERM => {
                    done.send(EventUpdate::Stop).unwrap();
                }
                _ => unreachable!(),
            }
        }
    });

    let mut core = Core::new();

    let mut started = vec![];
    loop {
        match read_config() {
            Ok(new) => cfg = new,
            Err(e) => {
                warn!(
                    "main: new configuration has an error ('{}'), reusing old config",
                    e
                );
            }
        };
        debug!("main: configuration: {:#?}", cfg);

        started = plugin::start_from_config(cfg.plugins.clone(), started)?;
        debug!("main: started.len()={}", started.len());

        let exit = core
            .run(&e2rx, &started)
            .context("main: core did not exit successfully")?;

        if let CoreExit::Stop = exit {
            for p in started {
                p.stop()?;
            }

            return Ok(());
        }
    }
}

fn min_drbd_versions() -> Result<()> {
    let version = Command::new("drbdadm").arg("--version").output()?;
    if !version.status.success() {
        return Err(anyhow::anyhow!(
            "'drbdadm --version' not executed successfully, stdout: '{}', stderr: '{}'",
            String::from_utf8(version.stdout).unwrap_or("<Could not convert stdout>".to_string()),
            String::from_utf8(version.stderr).unwrap_or("<Could not convert stderr>".to_string())
        ));
    }

    // check utils
    // secondary --force
    let pattern = Regex::new(r"^DRBDADM_VERSION_CODE=0x([[:xdigit:]]+)$")?;
    let (major, minor, patch) = split_version(pattern, version.stdout.clone())?;
    if let Err(e) = min_version((major, minor, patch), (9, 21, 2)) {
        return Err(anyhow::anyhow!(
            "drbdsetup minimum version ('9.21.2') not fulfilled: {}",
            e
        ));
    }

    // minimal kernel module version
    // secondary --force
    let pattern = Regex::new(r"^DRBD_KERNEL_VERSION_CODE=0x([[:xdigit:]]+)$")?;
    let (major, minor, patch) = split_version(pattern, version.stdout)?;
    if let Err(e) = min_version((major, minor, patch), (9, 1, 7)) {
        return Err(anyhow::anyhow!(
            "kernel module minimum version ('9.1.7') not fulfilled: {}",
            e
        ));
    }

    Ok(())
}

fn split_version(pattern: regex::Regex, stdout: Vec<u8>) -> Result<(u8, u8, u8)> {
    let version = String::from_utf8(stdout)?;
    let version = version
        .lines()
        .filter_map(|line| pattern.captures(line))
        .next()
        .ok_or(anyhow::anyhow!(
            "Could not determine version from pattern '{}'",
            pattern
        ))?;

    let version = u32::from_str_radix(&version[1], 16)?;

    let major = ((version >> 16) & 0xff) as u8;
    let minor = ((version >> 8) & 0xff) as u8;
    let patch = (version & 0xff) as u8;

    Ok((major, minor, patch))
}

fn min_version(have: (u8, u8, u8), want: (u8, u8, u8)) -> Result<()> {
    if have.0 > want.0 {
        return Ok(());
    }
    if have.0 < want.0 {
        return Err(anyhow::anyhow!(
            "Major version too small {} vs. {}",
            have.0,
            want.0
        ));
    }

    if have.1 > want.1 {
        return Ok(());
    }
    if have.1 < want.1 {
        return Err(anyhow::anyhow!(
            "Minor version too small {} vs. {}",
            have.1,
            want.1
        ));
    }

    if have.2 > want.2 {
        return Ok(());
    }
    if have.2 < want.2 {
        return Err(anyhow::anyhow!(
            "Patch version too small {} vs. {}",
            have.2,
            want.2
        ));
    }

    Ok(())
}
