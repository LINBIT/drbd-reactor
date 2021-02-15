use std::process::{Command, ExitStatus};
use std::sync::{mpsc, Arc};
use std::thread;

use anyhow::Result;
use log::info;
use serde::{Deserialize, Serialize};

use crate::drbd::{EventType, PluginUpdate};

pub mod debugger;
pub mod promoter;

pub type PluginSender = mpsc::Sender<Arc<PluginUpdate>>;
pub type PluginReceiver = mpsc::Receiver<Arc<PluginUpdate>>;

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
}

/// Start every enable plugin in its own thread and return a thread handle and the send end
/// of the channel used to communicate with the plugin.
pub fn start_from_config(
    cfg: PluginConfig,
) -> (Vec<thread::JoinHandle<Result<()>>>, Vec<PluginSender>) {
    let mut handles = Vec::new();
    let mut senders = Vec::new();

    for debug_cfg in cfg.debugger {
        let (ptx, prx) = mpsc::channel();
        let handle = thread::spawn(|| debugger::run(debug_cfg, prx));
        handles.push(handle);
        senders.push(ptx);
    }

    for promote_cfg in cfg.promoter {
        let (ptx, prx) = mpsc::channel();
        let handle = thread::spawn(|| promoter::run(promote_cfg, prx));
        handles.push(handle);
        senders.push(ptx);
    }

    (handles, senders)
}
