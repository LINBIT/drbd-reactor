use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::os::unix::net::UnixDatagram;
use std::process::{Command, ExitStatus};
use std::sync::Arc;
use std::{any, env, thread};

use anyhow::Result;
use log::{error, info, trace, warn};
use serde::{Deserialize, Serialize};

use crate::drbd::{EventType, PluginUpdate};

pub mod debugger;
pub mod prometheus;
pub mod promoter;
pub mod umh;

pub type PluginSender = crossbeam_channel::Sender<Arc<PluginUpdate>>;
pub type PluginReceiver = crossbeam_channel::Receiver<Arc<PluginUpdate>>;

trait Plugin: Send {
    fn run(&self, rx: PluginReceiver) -> anyhow::Result<()>;
    fn get_config(&self) -> PluginCfg;
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
    info!("system: sh -c {}", action);
    map_status(Command::new("sh").arg("-c").arg(action).status())
}

/// Central config for all available plugins.
///
/// Each plugin can be configured multiple times (hence the Vec everywhere), and each config item is
/// wrapped in a [crate::config::Component] to make it easy to disable plugins.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct PluginConfig {
    #[serde(default)]
    pub promoter: Vec<promoter::PromoterConfig>,
    #[serde(default)]
    pub debugger: Vec<debugger::DebuggerConfig>,
    #[serde(default)]
    pub umh: Vec<umh::UMHConfig>,
    #[serde(default)]
    pub prometheus: Vec<prometheus::PrometheusConfig>,
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum PluginCfg {
    Promoter(promoter::PromoterConfig),
    Debugger(debugger::DebuggerConfig),
    UMH(umh::UMHConfig),
    Prometheus(prometheus::PrometheusConfig),
}

pub struct PluginStarted {
    pub tx: PluginSender,
    pub handle: thread::JoinHandle<Result<()>>,
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

fn try_insert_unique(set: &mut HashSet<PluginCfg>, cfg: PluginCfg) -> Result<()> {
    if !set.insert(cfg.clone()) {
        return Err(anyhow::anyhow!(
            "duplicate config: '{:?}' was used multiple times",
            cfg
        ));
    }

    Ok(())
}

/// Start every enable plugin in its own thread and return a thread handle and the send end
/// of the channel used to communicate with the plugin.
pub fn start_from_config(
    cfg: PluginConfig,
    started: &mut HashMap<PluginCfg, PluginStarted>,
) -> Result<()> {
    let mut new_cfgs = HashSet::new();

    for p in &cfg.debugger {
        try_insert_unique(&mut new_cfgs, PluginCfg::Debugger(p.clone()))?;
    }
    for p in &cfg.promoter {
        try_insert_unique(&mut new_cfgs, PluginCfg::Promoter(p.clone()))?;
    }
    for p in &cfg.umh {
        try_insert_unique(&mut new_cfgs, PluginCfg::UMH(p.clone()))?;
    }
    for p in &cfg.prometheus {
        try_insert_unique(&mut new_cfgs, PluginCfg::Prometheus(p.clone()))?;
    }

    let mut survive = HashMap::new();
    for (cfg, mut plugin) in started.drain() {
        if new_cfgs.remove(&cfg) {
            // started and exists in new cfg -> retain
            trace!("start_from_config: keeping old config '{:#?}'", cfg);
            plugin.new = false;
            survive.insert(cfg, plugin);
        } else {
            // started, but not in new config -> stop
            trace!("start_from_config: stopping old config '{:#?}'", cfg);
            plugin.stop()?;
        }
    }
    *started = survive;

    let mut change_plugins: Vec<Box<dyn Plugin>> = Vec::new();
    let mut event_plugins: Vec<Box<dyn Plugin>> = Vec::new();

    for cfg in new_cfgs {
        deprecate_id(&cfg);
        trace!("start_from_config: starting new config '{:#?}'", cfg);
        match cfg {
            PluginCfg::Debugger(cfg) => match debugger::Debugger::new(cfg) {
                Ok(p) => change_plugins.push(Box::new(p)),
                Err(e) => error!(
                    "start_from_config: Could not start debugger plugin, ignoring it: {:#}",
                    e
                ),
            },
            PluginCfg::Promoter(cfg) => match promoter::Promoter::new(cfg) {
                Ok(p) => change_plugins.push(Box::new(p)),
                Err(e) => error!(
                    "start_from_config: Could not start promoter plugin, ignoring it: {:#}",
                    e
                ),
            },
            PluginCfg::UMH(cfg) => match umh::UMH::new(cfg) {
                Ok(p) => change_plugins.push(Box::new(p)),
                Err(e) => error!(
                    "start_from_config: Could not start umh plugin, ignoring it: {:#}",
                    e
                ),
            },
            PluginCfg::Prometheus(cfg) => match prometheus::Prometheus::new(cfg) {
                Ok(p) => event_plugins.push(Box::new(p)),
                Err(e) => error!(
                    "start_from_config: Could not start prometheus plugin, ignoring it: {:#}",
                    e
                ),
            },
        }
    }

    maybe_systemd_notify_ready()?;

    for d in change_plugins {
        let cfg = d.get_config();
        let (ptx, prx) = crossbeam_channel::unbounded();
        let handle = thread::spawn(move || d.run(prx));
        started.insert(
            cfg,
            PluginStarted {
                new: true,
                handle,
                tx: ptx,
                ptype: PluginType::Change,
            },
        );
    }

    for d in event_plugins {
        let cfg = d.get_config();
        let (ptx, prx) = crossbeam_channel::unbounded();
        let handle = thread::spawn(move || d.run(prx));
        started.insert(
            cfg,
            PluginStarted {
                new: true,
                handle,
                tx: ptx,
                ptype: PluginType::Event,
            },
        );
    }

    Ok(())
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

fn deprecate_id(cfg: &PluginCfg) {
    let warn = || {
        warn!("'id' is deprecated and ignored!");
    };

    match cfg {
        PluginCfg::Debugger(cfg) if cfg.id.is_some() => warn(),
        PluginCfg::Promoter(cfg) if cfg.id.is_some() => warn(),
        PluginCfg::UMH(cfg) if cfg.id.is_some() => warn(),
        PluginCfg::Prometheus(cfg) if cfg.id.is_some() => warn(),
        PluginCfg::Debugger(_)
        | PluginCfg::Promoter(_)
        | PluginCfg::UMH(_)
        | PluginCfg::Prometheus(_) => (),
    }
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
