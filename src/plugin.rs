use std::env;
use std::os::unix::net::UnixDatagram;
use std::process::{Command, ExitStatus};
use std::sync::{mpsc, Arc};
use std::thread;

use anyhow::Result;
use log::info;
use serde::{Deserialize, Serialize};

use crate::drbd::{EventType, PluginUpdate};

pub mod debugger;
pub mod promoter;
pub mod umh;

pub type PluginSender = mpsc::Sender<Arc<PluginUpdate>>;
pub type PluginReceiver = mpsc::Receiver<Arc<PluginUpdate>>;

trait Plugin: Send {
    fn run(&self, rx: PluginReceiver) -> anyhow::Result<()>;
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
#[derive(Deserialize, Serialize, Debug)]
pub struct PluginConfig {
    #[serde(default)]
    promoter: Vec<promoter::PromoterConfig>,
    #[serde(default)]
    debugger: Vec<debugger::DebuggerConfig>,
    #[serde(default)]
    umh: Vec<umh::UMHConfig>,
}

/// Start every enable plugin in its own thread and return a thread handle and the send end
/// of the channel used to communicate with the plugin.
pub fn start_from_config(
    cfg: PluginConfig,
) -> Result<(Vec<thread::JoinHandle<Result<()>>>, Vec<PluginSender>)> {
    let mut handles = Vec::new();
    let mut senders = Vec::new();

    let mut plugins: Vec<Box<dyn Plugin>> = Vec::new();
    for debug_cfg in cfg.debugger {
        plugins.push(Box::new(debugger::Debugger::new(debug_cfg)?));
    }
    for promote_cfg in cfg.promoter {
        plugins.push(Box::new(promoter::Promoter::new(promote_cfg)?));
    }
    for umh_cfg in cfg.umh {
        plugins.push(Box::new(umh::UMH::new(umh_cfg)?));
    }

    maybe_systemd_notify_ready()?;

    for d in plugins {
        let (ptx, prx) = mpsc::channel();
        let handle = thread::spawn(move || d.run(prx));
        handles.push(handle);
        senders.push(ptx);
    }

    Ok((handles, senders))
}

fn maybe_systemd_notify_ready() -> Result<()> {
    let socket = match env::var_os("NOTIFY_SOCKET") {
        Some(socket) => socket,
        None => return Ok(()),
    };

    let sock = UnixDatagram::unbound()?;
    let msg = "READY=1\n";
    if sock.send_to(msg.as_bytes(), socket)? != msg.len() {
        Err(anyhow::anyhow!(
            "Could not completely write 'READY=1' to NOTIFY_SOCKET"
        ))
    } else {
        Ok(())
    }
}
