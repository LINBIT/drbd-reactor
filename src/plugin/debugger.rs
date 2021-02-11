use crate::drbd::PluginUpdate;
use anyhow::Result;
use log::{debug, trace};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

pub fn run(_cfg: DebuggerOpt, rx: Receiver<Arc<PluginUpdate>>) -> Result<()> {
    trace!("debugger: start");

    for r in rx {
        debug!("{:#?}", r);
    }

    trace!("debugger: exit");
    Ok(())
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DebuggerOpt {}
