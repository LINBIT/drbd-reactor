use anyhow::Result;
use log::{debug, trace};
use serde::{Deserialize, Serialize};

use crate::plugin::PluginCfg;

pub struct Debugger {
    cfg: DebuggerConfig,
}

impl Debugger {
    pub fn new(cfg: DebuggerConfig) -> Result<Self> {
        Ok(Debugger { cfg })
    }
}

impl super::Plugin for Debugger {
    fn run(&self, rx: super::PluginReceiver) -> Result<()> {
        trace!("run: start");

        for r in rx {
            debug!("{:#?}", r);
        }

        trace!("run: exit");
        Ok(())
    }

    fn get_config(&self) -> PluginCfg {
        PluginCfg::Debugger(self.cfg.clone())
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone, Default)]
pub struct DebuggerConfig {
    pub id: Option<String>,
}
