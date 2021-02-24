use anyhow::Result;
use log::{debug, trace};
use serde::{Deserialize, Serialize};

pub struct Debugger {}

impl Debugger {
    pub fn new(_cfg: DebuggerConfig) -> Result<Self> {
        Ok(Debugger {})
    }
}

impl super::Plugin for Debugger {
    fn run(&self, rx: super::PluginReceiver) -> Result<()> {
        trace!("debugger: start");

        for r in rx {
            debug!("{:#?}", r);
        }

        trace!("debugger: exit");
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DebuggerConfig {}
