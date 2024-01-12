use std::collections::HashMap;
use std::fs::read_to_string;
use std::path::PathBuf;
use std::time::Duration;
use std::{io, sync, thread};

use anyhow::{Context, Result};

use log::{debug, error, warn};
use signal_hook::iterator::Signals;
use structopt::StructOpt;

use drbd_reactor::drbd;
use drbd_reactor::drbd::{EventType, EventUpdate, PluginUpdate, Resource};
use drbd_reactor::events::events2;
use drbd_reactor::{config, plugin};

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

#[derive(PartialEq)]
enum CoreExit {
    Stop,
    Reload,
    Flush,
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
        e2rx: &crossbeam_channel::Receiver<EventUpdate>,
        started: &HashMap<plugin::PluginCfg, plugin::PluginStarted>,
    ) -> Result<CoreExit> {
        let _send_updates = |up: Option<PluginUpdate>,
                             res: &Resource,
                             et: &EventType,
                             only_new: bool|
         -> Result<()> {
            if let Some(up) = up {
                let up = sync::Arc::new(up);
                for p in started.values() {
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
            for p in started.values() {
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
                    send_updates(up, res, &et)?;

                    if et == EventType::Destroy {
                        self.resources.remove(&r.name);
                    }
                }
                EventUpdate::Device(et, d) => {
                    let res = self.get_or_create_resource(&d.name);
                    let up = res.get_device_update(&et, &d);
                    send_updates(up, res, &EventType::Change)?;
                }
                EventUpdate::PeerDevice(et, pd) => {
                    let res = self.get_or_create_resource(&pd.name);
                    let up = res.get_peerdevice_update(&et, &pd);
                    send_updates(up, res, &EventType::Change)?;
                }
                EventUpdate::Connection(et, c) => {
                    let res = self.get_or_create_resource(&c.name);
                    let up = res.get_connection_update(&et, &c);
                    send_updates(up, res, &EventType::Change)?;
                }
                EventUpdate::Path(et, p) => {
                    let res = self.get_or_create_resource(&p.name);
                    let up = res.get_path_update(&et, &p);
                    send_updates(up, res, &EventType::Change)?;
                }
                EventUpdate::Stop => return Ok(CoreExit::Stop),
                EventUpdate::Reload => return Ok(CoreExit::Reload),
                EventUpdate::Flush => return Ok(CoreExit::Flush),
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
    let cli_opt = CliOpt::from_args();

    let tty = atty::is(atty::Stream::Stdin)
        || atty::is(atty::Stream::Stdout)
        || atty::is(atty::Stream::Stderr);
    if tty && !cli_opt.allow_tty {
        return Err(anyhow::anyhow!(
            "Refusing to start in a terminal without --allow-tty. Did you mean drbd-reactorctl?"
        ));
    }

    let mut cfg = get_config(&cli_opt.config)?;
    init_loggers(cfg.clone().log)?;

    let (e2tx, e2rx) = crossbeam_channel::unbounded();

    setup_signals(e2tx.clone())?;

    let statistics_poll = Duration::from_secs(cfg.statistics_poll_interval);
    thread::spawn(move || {
        if let Err(e) = events2(e2tx, statistics_poll) {
            error!("main: events2 processing failed: {}", e);
            std::process::exit(1);
        }
    });

    let mut core = Core::new();

    let mut started = HashMap::new();
    loop {
        match get_config(&cli_opt.config) {
            Ok(new) => cfg = new,
            Err(e) => warn!("main: failed to reload config, reusing old: {}", e),
        };
        debug!("main: configuration: {:#?}", cfg);

        plugin::start_from_config(cfg.plugins.clone(), &mut started)?;
        debug!("main: started.len()={}", started.len());

        let reason = core
            .run(&e2rx, &started)
            .context("main: core did not exit successfully")?;

        match reason {
            CoreExit::Stop => {
                for (_, plugin) in started.drain() {
                    plugin.stop()?;
                }
                return Ok(());
            }
            CoreExit::Flush => {
                for (_, plugin) in started.drain() {
                    plugin.stop()?;
                }
                core.resources.clear();
            }
            CoreExit::Reload => (),
        }
    }
}

fn setup_signals(events: crossbeam_channel::Sender<EventUpdate>) -> Result<()> {
    let mut signals = Signals::new(&[libc::SIGHUP, libc::SIGINT, libc::SIGTERM])?;
    debug!("signal-handler: set up done");

    thread::spawn(move || {
        debug!("signal-handler: waiting for signals");
        for signal in signals.forever() {
            let event = match signal as libc::c_int {
                libc::SIGHUP => EventUpdate::Reload,
                libc::SIGINT | libc::SIGTERM => EventUpdate::Stop,
                _ => unreachable!(),
            };

            if let Err(e) = events.send(event) {
                error!("signal-handler: failed to send events: {}", e);
                std::process::exit(1);
            }
        }
    });

    Ok(())
}

fn get_config(config_file: &PathBuf) -> Result<config::Config> {
    match read_config(config_file) {
        Ok(new) if !new.plugins.promoter.is_empty() => {
            min_drbd_versions()?;
            Ok(new)
        }
        x => x,
    }
}

fn min_drbd_versions() -> Result<()> {
    let drbd_versions = drbd::get_drbd_versions()?;

    // check utils
    // proper events2 --poll termination
    let want = drbd::Version {
        major: 9,
        minor: 26,
        patch: 0,
    };
    if drbd_versions.utils < want {
        return Err(anyhow::anyhow!(
            "drbdsetup minimum version ('{}') not fulfilled by '{}'",
            want,
            drbd_versions.utils
        ));
    }

    // minimal kernel module version
    // secondary --force
    let kmod = drbd_versions.kmod;
    if kmod.major == 0 && kmod.minor == 0 && kmod.patch == 0 {
        return Err(anyhow::anyhow!(
            "Looks like the DRBD kernel module is not installed or not loaded"
        ));
    }
    let want = drbd::Version {
        major: 9,
        minor: 1,
        patch: 7,
    };
    if kmod < want {
        return Err(anyhow::anyhow!(
            "DRBD kernel module minimum version ('{}') not fulfilled by '{}'",
            want,
            kmod
        ));
    }

    Ok(())
}

#[derive(Debug, StructOpt)]
struct CliOpt {
    #[structopt(
        short,
        long,
        parse(from_os_str),
        default_value = "/etc/drbd-reactor.toml"
    )]
    config: PathBuf,
    #[structopt(long)]
    allow_tty: bool,
}

fn read_config(config_file: &PathBuf) -> Result<config::Config> {
    // as we also need the content of the main config in the daemon config, we don't use config::get_snippets_path
    let mut content = read_to_string(config_file)
        .with_context(|| format!("Could not read config file: {}", config_file.display()))?;

    let mut config: config::Config = toml::from_str(&content).with_context(|| {
        format!(
            "Could not parse main config file; content: {}",
            config_file.display()
        )
    })?;

    let snippets_path = match config.snippets {
        None => return Ok(config),
        Some(path) => path,
    };

    let snippets_paths = config::files_with_extension_in(&snippets_path, "toml")?;
    let snippets = config::read_snippets(&snippets_paths)
        .with_context(|| "Could not read config snippets".to_string())?;
    content.push_str("\n# Content from snippets:\n");
    content.push_str(&snippets);
    config = toml::from_str(&content).with_context(|| {
        format!(
            "Could not parse config files including snippets; content: {}",
            content
        )
    })?;

    Ok(config)
}
