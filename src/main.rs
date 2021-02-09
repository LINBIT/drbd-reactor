use anyhow::Result;
use drbdd::drbd::{EventType, EventUpdate, PluginUpdate, Resource};
use drbdd::events::events2;
use drbdd::plugin::{debugger, promoter};
use log::error;
use std::collections::HashMap;
use std::sync::mpsc::channel;
use std::thread;

struct Core {
    resources: HashMap<String, Resource>,
    config: drbdd::config::ConfigOpt,
}

impl Core {
    fn new(cfg: drbdd::config::ConfigOpt) -> Core {
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
                    } else {
                        res.update(&r);
                    }
                }
                EventUpdate::DeviceUpdate(et, d) => {
                    let res = self.get_or_create_resource(&d.name);

                    if let Some(i) = res.get_device_update(&et, &d) {
                        send_to_event_plugins(i)?;
                    }
                    res.update_or_delete_device(&et, d);
                }
                EventUpdate::PeerDeviceUpdate(et, pd) => {
                    let res = self.get_or_create_resource(&pd.name);

                    if let Some(i) = res.get_peerdevice_update(&et, &pd) {
                        send_to_event_plugins(i)?;
                    }
                    res.update_or_delete_peerdevice(&et, pd);
                }
                EventUpdate::ConnectionUpdate(et, c) => {
                    let res = self.get_or_create_resource(&c.name);

                    if let Some(i) = res.get_connection_update(&et, &c) {
                        send_to_event_plugins(i)?;
                    }
                    res.update_or_delete_connection(&et, c)
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

fn main() {
    let cfg = drbdd::config::from_args().expect("Config file does not exist or is invalid");

    // TODO(): maybe better handle this with enums
    let verbosity = match &cfg.log.level[..] {
        "trace" => 4,
        "debug" => 3,
        "info" => 2,
        "warn" => 1,
        "error" => 0,
        _ => 0,
    };
    let timestamps = match &cfg.log.timestamps[..] {
        "sec" => stderrlog::Timestamp::Second,
        "ms" => stderrlog::Timestamp::Millisecond,
        "us" => stderrlog::Timestamp::Microsecond,
        "ns" => stderrlog::Timestamp::Nanosecond,
        _ => stderrlog::Timestamp::Off,
    };

    stderrlog::new()
        .module(module_path!())
        .quiet(cfg.log.quiet)
        .verbosity(verbosity)
        .timestamp(timestamps)
        .init()
        .unwrap();

    Core::new(cfg)
        .run()
        .expect("core did not exit successfully");

    std::process::exit(exitcode::OK);
}
