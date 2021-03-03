use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use log::{info, trace, warn};
use serde::{Deserialize, Serialize};

use crate::drbd::{EventType, PluginUpdate};
use crate::plugin;

pub struct Promoter {
    cfg: PromoterConfig,
}

impl Promoter {
    pub fn new(cfg: PromoterConfig) -> Result<Self> {
        let names = cfg.resources.keys().cloned().collect::<Vec<String>>();
        adjust_resources(&names)?;

        Ok(Self { cfg })
    }
}

impl super::Plugin for Promoter {
    fn run(&self, rx: super::PluginReceiver) -> Result<()> {
        trace!("promoter: start");

        let type_exists = plugin::typefilter(&EventType::Exists);
        let type_change = plugin::typefilter(&EventType::Change);
        let names = self.cfg.resources.keys().cloned().collect::<Vec<String>>();
        let names = plugin::namefilter(&names);

        // set default stop actions (i.e., reversed start, and default on-stop-failure (i.e., true)
        let cfg = {
            let mut cfg = self.cfg.clone();
            for res in cfg.resources.values_mut() {
                if res.stop.is_empty() {
                    res.stop = res.start.clone();
                    res.stop.reverse();
                }
                if res.on_stop_failure == "" {
                    res.on_stop_failure = "true".to_string();
                }
            }
            cfg
        };

        for r in rx
            .into_iter()
            .filter(names)
            .filter(|x| type_exists(x) || type_change(x))
        {
            let name = r.get_name();
            let res = cfg
                .resources
                .get(&name)
                .expect("Can not happen, name filter is built from the cfg");

            match r.as_ref() {
                PluginUpdate::ResourceUpdate(_, u) => {
                    if !u.old.may_promote && u.new.may_promote {
                        info!("promoter: resource '{}' may promote", name);
                        if start_actions(&res.start).is_err() && stop_actions(&res.stop).is_err() {
                            on_failure(&res.on_stop_failure); // loops until success
                        }
                    }
                }
                PluginUpdate::DeviceUpdate(_, u) => {
                    if u.old.quorum && !u.new.quorum {
                        info!("promoter: resource '{}' lost quorum", name);
                        if stop_actions(&res.stop).is_err() {
                            on_failure(&res.on_stop_failure); // loops until success
                        }
                    }
                }
                _ => (),
            }
        }

        trace!("promoter: exit");
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct PromoterConfig {
    #[serde(default)]
    pub resources: HashMap<String, PromoterOptResource>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct PromoterOptResource {
    #[serde(default)]
    pub start: Vec<String>,
    #[serde(default)]
    pub stop: Vec<String>,
    #[serde(default)]
    pub on_stop_failure: String,
}

fn systemd_stop(unit: &str) -> Result<()> {
    info!("promoter: systemctl stop {}", unit);
    plugin::map_status(Command::new("systemctl").arg("stop").arg(unit).status())
}

fn systemd_start(unit: &str) -> Result<()> {
    // we really don't care
    let _ = Command::new("systemctl")
        .arg("reset-failed")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg(unit)
        .status();

    info!("promoter: systemctl start {}", unit);
    plugin::map_status(Command::new("systemctl").arg("start").arg(unit).status())
}

fn action(to: State, what: &str) -> Result<()> {
    let words = what.split_whitespace().collect::<Vec<&str>>();
    if words.is_empty() {
        return Err(anyhow::anyhow!("action is empty"));
    }

    if Path::new(words[0]).is_absolute() {
        return plugin::system(what);
    }

    match to {
        State::Start => systemd_start(what),
        State::Stop => systemd_stop(what),
    }
}

fn start_actions(actions: &[String]) -> Result<()> {
    for a in actions {
        action(State::Start, a)?;
    }
    Ok(())
}

fn stop_actions(actions: &[String]) -> Result<()> {
    for a in actions {
        action(State::Stop, a)?;
    }
    Ok(())
}

pub fn on_failure(action: &str) {
    info!("promoter: starting on-failure action in a loop");
    loop {
        if plugin::system(action).is_ok() {
            return;
        }
        thread::sleep(Duration::from_secs(2));
    }
}

fn adjust_resources(to_start: &[String]) -> Result<()> {
    for res in to_start {
        let status = Command::new("drbdadm").arg("adjust").arg(res).status()?;
        if !status.success() {
            // for now let's keep it a warning, I don't think we should fail hard here.
            warn!(
                "promoter: 'drbdadm adjust {}' did not return successfully",
                res
            );
        }
    }
    Ok(())
}

enum State {
    Start,
    Stop,
}
