use anyhow::Result;
use log::{debug, trace};
use serde::{Deserialize, Serialize};

pub fn run(_cfg: DebuggerConfig, rx: super::PluginReceiver) -> Result<()> {
    trace!("debugger: start");

    for r in rx {
        debug!("{:#?}", r);
    }

    trace!("debugger: exit");
    Ok(())
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DebuggerConfig {}
