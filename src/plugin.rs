pub mod debugger;
pub mod promoter;
use crate::drbd::{EventType, PluginUpdate};
use anyhow::Result;
use log::info;
use std::process::{Command, ExitStatus};

pub fn namefilter<'a>(names: &'a [String]) -> impl Fn(&PluginUpdate) -> bool + 'a {
    return move |up: &PluginUpdate| {
        for name in names {
            if up.has_name(name) {
                return true;
            }
        }
        return false;
    };
}

pub fn typefilter<'a>(ftype: &'a EventType) -> impl Fn(&PluginUpdate) -> bool + 'a {
    return move |up: &PluginUpdate| up.has_type(ftype);
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
