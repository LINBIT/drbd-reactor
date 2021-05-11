use std::collections::{HashMap, HashSet};
use std::os::unix::net::UnixDatagram;
use std::process::{Command, ExitStatus};
use std::sync::{mpsc, Arc};
use std::{any, env, thread};

use anyhow::Result;
use log::{debug, info};
use serde::{Deserialize, Serialize};

use crate::drbd::{EventType, PluginUpdate};

pub mod debugger;
pub mod prometheus;
pub mod promoter;
pub mod umh;

pub type PluginSender = mpsc::Sender<Arc<PluginUpdate>>;
pub type PluginReceiver = mpsc::Receiver<Arc<PluginUpdate>>;

trait Plugin: Send {
    fn run(&self, rx: PluginReceiver) -> anyhow::Result<()>;
    fn get_id(&self) -> Option<String>;
}

pub fn namefilter(names: &[String]) -> impl Fn(&Arc<PluginUpdate>) -> bool + '_ {
    return move |up: &Arc<PluginUpdate>| {
        for name in names {
            if up.has_name(name) {
                return true;
            }
        }
        return false;
    };
}

pub fn typefilter(ftype: &EventType) -> impl Fn(&Arc<PluginUpdate>) -> bool + '_ {
    return move |up: &Arc<PluginUpdate>| up.has_type(ftype);
}

pub fn map_status(status: std::result::Result<ExitStatus, std::io::Error>) -> Result<()> {
    match status {
        Ok(status) => {
            if status.success() {
                Ok(())
            } else {
                return Err(anyhow::anyhow!("Return code not status success"));
            }
        }
        Err(e) => Err(anyhow::anyhow!("Could not execute: {}", e)),
    }
}

pub fn system(action: &str) -> Result<()> {
    info!("promoter: sh -c {}", action);
    map_status(Command::new("sh").arg("-c").arg(action).status())
}

/// Central config for all available plugins.
///
/// Each plugin can be configured multiple times (hence the Vec everywhere), and each config item is
/// wrapped in a [crate::config::Component] to make it easy to disable plugins.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct PluginConfig {
    #[serde(default)]
    promoter: Vec<promoter::PromoterConfig>,
    #[serde(default)]
    debugger: Vec<debugger::DebuggerConfig>,
    #[serde(default)]
    umh: Vec<umh::UMHConfig>,
    #[serde(default)]
    prometheus: Vec<prometheus::PrometheusConfig>,
}

pub struct PluginStarted {
    pub tx: PluginSender,
    pub handle: thread::JoinHandle<Result<()>>,
    id: Option<String>,
    pub new: bool,
    pub ptype: PluginType,
}
pub enum PluginType {
    Change, // important changes
    Event,  // every event line
}
impl PluginStarted {
    pub fn stop(self) -> Result<()> {
        drop(self.tx);
        self.handle
            .join()
            .unwrap_or_else(|e| Err(thread_panic_error(e)))
    }
}

fn stop_not_in_new(
    started: Vec<PluginStarted>,
    new_ids: &HashSet<String>,
) -> Result<Vec<PluginStarted>> {
    let mut filtered = Vec::new();

    for mut p in started {
        if let Some(ref id) = p.id {
            let in_new = new_ids.get(id).is_some();
            if in_new {
                p.new = false;
                filtered.push(p);
            } else {
                debug!(
                    "plugin: stop_not_in_new: stopping plugin with ID '{}'",
                    id.to_string()
                );
                p.stop()?
            }
        } else {
            debug!("plugin: stop_not_in_new: stopping plugin without ID");
            p.stop()?
        }
    }

    Ok(filtered)
}

fn start_new_plugin(id: &Option<String>, survived: &HashMap<String, bool>) -> bool {
    match id {
        Some(id) => survived.get(id).is_none(),
        None => true,
    }
}

fn unique_plugin_id(set: &mut HashSet<String>, id: &str) -> Result<()> {
    if !set.insert(id.to_string()) {
        return Err(anyhow::anyhow!("ID '{}' was used multiple times", id));
    }

    Ok(())
}

/// Start every enable plugin in its own thread and return a thread handle and the send end
/// of the channel used to communicate with the plugin.
pub fn start_from_config(
    cfg: PluginConfig,
    started: Vec<PluginStarted>,
) -> Result<Vec<PluginStarted>> {
    let mut new_ids = HashSet::new();

    for id in cfg.debugger.iter().filter_map(|c| c.id.as_ref()) {
        unique_plugin_id(&mut new_ids, id)?;
    }
    for id in cfg.promoter.iter().filter_map(|c| c.id.as_ref()) {
        unique_plugin_id(&mut new_ids, id)?;
    }
    for id in cfg.umh.iter().filter_map(|c| c.id.as_ref()) {
        unique_plugin_id(&mut new_ids, id)?;
    }
    for id in cfg.prometheus.iter().filter_map(|c| c.id.as_ref()) {
        unique_plugin_id(&mut new_ids, id)?;
    }

    let mut started = stop_not_in_new(started, &new_ids)?;

    let mut survived: HashMap<String, bool> = HashMap::new();
    for p in &started {
        match &p.id {
            Some(id) => {
                survived.insert(id.to_string(), true);
                debug!(
                    "plugin: start_from_config: plugin with ID '{}' survived",
                    id.to_string()
                );
            }
            None => {
                return Err(anyhow::anyhow!(
                    "plugin: start_from_config: found started id==None plugin"
                ));
            }
        }
    }

    let mut change_plugins: Vec<Box<dyn Plugin>> = Vec::new();
    for debug_cfg in cfg.debugger {
        if start_new_plugin(&debug_cfg.id, &survived) {
            change_plugins.push(Box::new(debugger::Debugger::new(debug_cfg)?));
        }
    }
    for promote_cfg in cfg.promoter {
        if start_new_plugin(&promote_cfg.id, &survived) {
            change_plugins.push(Box::new(promoter::Promoter::new(promote_cfg)?));
        }
    }
    for umh_cfg in cfg.umh {
        if start_new_plugin(&umh_cfg.id, &survived) {
            change_plugins.push(Box::new(umh::UMH::new(umh_cfg)?));
        }
    }

    let mut event_plugins: Vec<Box<dyn Plugin>> = Vec::new();
    for prometheus_cfg in cfg.prometheus {
        if start_new_plugin(&prometheus_cfg.id, &survived) {
            event_plugins.push(Box::new(prometheus::Prometheus::new(prometheus_cfg)?));
        }
    }

    maybe_systemd_notify_ready()?;

    for d in change_plugins {
        let id = d.get_id();
        let (ptx, prx) = mpsc::channel();
        let handle = thread::spawn(move || d.run(prx));
        started.push(PluginStarted {
            id,
            new: true,
            handle,
            tx: ptx,
            ptype: PluginType::Change,
        });
    }

    for d in event_plugins {
        let id = d.get_id();
        let (ptx, prx) = mpsc::channel();
        let handle = thread::spawn(move || d.run(prx));
        started.push(PluginStarted {
            id,
            new: true,
            handle,
            tx: ptx,
            ptype: PluginType::Event,
        });
    }

    Ok(started)
}

fn maybe_systemd_notify_ready() -> Result<()> {
    let key = "NOTIFY_SOCKET";
    let socket = match env::var_os(key) {
        Some(socket) => socket,
        None => return Ok(()),
    };

    env::remove_var(key);

    let sock = UnixDatagram::unbound()?;
    let msg = "READY=1\n";
    if sock.send_to(msg.as_bytes(), socket)? != msg.len() {
        Err(anyhow::anyhow!(
            "Could not completely write 'READY=1' to {}",
            key
        ))
    } else {
        Ok(())
    }
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
