use glob::glob;
use std::collections::HashMap;
use std::fs::read_to_string;
use std::path::PathBuf;
use std::time::Duration;
use std::{any, io, sync, thread};

use anyhow::{Context, Result};
use fern;
use log::error;
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
    }

    Ok(s)
}

pub fn from_args() -> Result<config::Config> {
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
    /// state of the world and forwarding this information to all plugins via the given senders.
    fn run(
        &mut self,
        change_plugin_txs: Vec<plugin::PluginSender>,
        event_plugin_txs: Vec<plugin::PluginSender>,
        statistics_poll: Duration,
    ) -> Result<()> {
        if change_plugin_txs.is_empty() && event_plugin_txs.is_empty() {
            return Err(anyhow::anyhow!("You need to enable at least one plugin"));
        }

        let send_to_plugins =
            |up: PluginUpdate, plugin_txs: &Vec<plugin::PluginSender>| -> Result<()> {
                let up = sync::Arc::new(up);
                for tx in plugin_txs {
                    tx.send(up.clone())?;
                }
                Ok(())
            };

        let send_updates =
            |up: Option<PluginUpdate>, res: &Resource, et: &EventType| -> Result<()> {
                if let Some(up) = up {
                    send_to_plugins(up, &change_plugin_txs)?;
                }
                send_to_plugins(
                    PluginUpdate::ResourceOnly(et.clone(), res.clone()),
                    &event_plugin_txs,
                )?;
                Ok(())
            };

        let (e2tx, e2rx) = sync::mpsc::channel();
        let done = e2tx.clone();
        thread::spawn(move || {
            if let Err(e) = events2(e2tx, statistics_poll) {
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
                EventUpdate::Stop => break,
            }
        }

        Ok(())
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

/// Converts a message generated by `panic!` into an error.
///
/// Useful to convert the Result of a thread handle `.join()` into a readable error message.
fn thread_panic_error(original: Box<dyn any::Any + Send>) -> anyhow::Error {
    match original.downcast_ref::<&str>() {
        Some(d) => return anyhow::anyhow!("plugin panicked: {}", d),
        None => (),
    };

    match original.downcast_ref::<String>() {
        Some(d) => return anyhow::anyhow!("plugin panicked: {}", d),
        None => (),
    };

    anyhow::anyhow!("plugin panicked with unrecoverable error message")
}

fn main() -> Result<()> {
    let cfg = from_args()?;

    init_loggers(cfg.log)?;

    let (handles, change_senders, event_senders) = plugin::start_from_config(cfg.plugins)?;
    let statistics_poll = Duration::from_secs(cfg.statistics_poll_interval);
    Core::new()
        .run(change_senders, event_senders, statistics_poll)
        .context("core did not exit successfully")?;

    for handle in handles {
        handle
            .join()
            .unwrap_or_else(|e| Err(thread_panic_error(e)))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panic_error() {
        let str_result = thread::spawn(|| panic!("some &str panic")).join();
        let panic_msg = str_result.expect_err("must panic");
        let panic_err = thread_panic_error(panic_msg);
        assert_eq!(panic_err.to_string(), "plugin panicked: some &str panic");

        let string_result = thread::spawn(|| panic!("some String panic: {}", 2)).join();
        let panic_msg = string_result.expect_err("must panic");
        let panic_err = thread_panic_error(panic_msg);
        assert_eq!(
            panic_err.to_string(),
            "plugin panicked: some String panic: 2"
        );
    }
}
