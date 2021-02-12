use std::collections::HashMap;
use std::fs::read_to_string;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::channel;
use std::thread;

use anyhow::{Context, Result};
use log::error;
use structopt::StructOpt;

use drbdd::config;
use drbdd::drbd::{EventType, EventUpdate, PluginUpdate, Resource};
use drbdd::events::events2;
use drbdd::plugin::{debugger, promoter};

#[derive(Debug, StructOpt)]
struct CliOpt {
    #[structopt(short, long, parse(from_os_str), default_value = "/etc/drbdd.toml")]
    config: PathBuf,
}

pub fn from_args() -> Result<config::ConfigOpt> {
    let cli_opt = CliOpt::from_args();

    let content = read_to_string(&cli_opt.config)
        .with_context(|| format!("Could not read config file: {}", cli_opt.config.display()))?;
    let config = toml::from_str(&content)
        .with_context(|| format!("Could not parse config file content: {}", cli_opt.config.display()))?;

    Ok(config)
}

/// Core handles DRBD events based on the provided configuration
///
/// It will
/// * start a listener thread, which runs "drbdsetup events2" and converts the events to structs
/// * start "plugin" threads, which expect to be notified about all state changes
/// * on the main thread, receive structs from the listener, keeping its state-of-the-world in
///   sync.
/// * Based on the struct received and the existing state, the main thread will forward the events
///   to the plugin threads, enhancing the raw event with additional information like
///   - the old state
///   - the new state
///   - the overall resource state
struct Core {
    resources: HashMap<String, Resource>,
    config: config::ConfigOpt,
}

impl Core {
    fn new(cfg: config::ConfigOpt) -> Core {
        Core {
            resources: HashMap::new(),
            config: cfg,
        }
    }

    fn get_or_create_resource(&mut self, name: &str) -> &mut Resource {
        self.resources
            .entry(name.into())
            .or_insert(Resource::with_name(name))
    }

    fn run(&mut self) -> Result<()> {
        let mut handles = vec![];
        let mut event_plugin_txs = vec![];

        for p in &self.config.plugins {
            let (ptx, prx) = channel();
            match p.as_ref() {
                "promoter" => {
                    let cfg = self.config.promoter.clone();
                    let handle = thread::spawn(|| promoter::run(cfg, prx));
                    handles.push(handle);
                    event_plugin_txs.push(ptx);
                }
                "debugger" => {
                    let cfg = self.config.debugger.clone();
                    let handle = thread::spawn(|| debugger::run(cfg, prx));
                    handles.push(handle);
                    event_plugin_txs.push(ptx);
                }
                _ => return Err(anyhow::anyhow!("unknown plugin")),
            }
        }

        if handles.is_empty() {
            return Err(anyhow::anyhow!("You need to enable at least one plugin"));
        }

        let send_to_event_plugins = |up: PluginUpdate| -> Result<()> {
            let up = Arc::new(up);
            for tx in &event_plugin_txs {
                tx.send(up.clone())?;
            }
            Ok(())
        };

        let (e2tx, e2rx) = channel();
        let done = e2tx.clone();
        thread::spawn(|| {
            if let Err(e) = events2(e2tx) {
                error!("core: events2 processing failed: {}", e);
                std::process::exit(1);
            }
        });

        ctrlc::set_handler(move || {
            println!("received Ctrl+C!");
            done.send(EventUpdate::Stop).unwrap();
        })?;

        for r in e2rx {
            match r {
                EventUpdate::ResourceUpdate(et, r) => {
                    let res = self.get_or_create_resource(&r.name);

                    if let Some(i) = res.get_resource_update(&et, &r) {
                        send_to_event_plugins(i)?;
                    }

                    if et == EventType::Destroy {
                        self.resources.remove(&r.name);
                    }
                }
                EventUpdate::DeviceUpdate(et, d) => {
                    let res = self.get_or_create_resource(&d.name);

                    if let Some(i) = res.get_device_update(&et, &d) {
                        send_to_event_plugins(i)?;
                    }
                }
                EventUpdate::PeerDeviceUpdate(et, pd) => {
                    let res = self.get_or_create_resource(&pd.name);

                    if let Some(i) = res.get_peerdevice_update(&et, &pd) {
                        send_to_event_plugins(i)?;
                    }
                }
                EventUpdate::ConnectionUpdate(et, c) => {
                    let res = self.get_or_create_resource(&c.name);

                    if let Some(i) = res.get_connection_update(&et, &c) {
                        send_to_event_plugins(i)?;
                    }
                }
                EventUpdate::Stop => break,
            }
        }

        for tx in event_plugin_txs {
            drop(tx);
        }
        for h in handles {
            if let Err(_) = h.join() {
                return Err(anyhow::anyhow!("plugin thread panicked"));
            }
        }

        Ok(())
    }
}

fn main() -> Result<()> {
    let cfg = from_args()?;

    stderrlog::new()
        .module(module_path!())
        .quiet(cfg.log.quiet)
        // There is no way to set the log level directly. Instead we have to
        // use this verbosity setting, which converts back to a LevelFilter
        // while ignoring the "Off" variant. For example,
        // LevelFilter::Error -> 1 as usize -> 0 verbosity.
        // LevelFilter::Off -> 0 as usize -> 0 verbosity.
        .verbosity((cfg.log.level as usize).saturating_sub(1))
        .timestamp(cfg.log.timestamps)
        .init()
        .context("failed to set up logger")?;

    Core::new(cfg)
        .run()
        .context("core did not exit successfully")?;

    Ok(())
}
