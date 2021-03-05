use anyhow::Result;
use log::{debug, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;
use std::thread;

use crate::drbd::{
    ConnectionPluginUpdatePattern, ConnectionUpdateStatePattern, DevicePluginUpdatePattern,
    DeviceUpdateStatePattern, EventType, PeerDevicePluginUpdatePattern,
    PeerDeviceUpdateStatePattern, PluginUpdate, ResourcePluginUpdatePattern,
    ResourceUpdateStatePattern,
};
use crate::matchable::{BasicPattern, PartialMatchable};

pub struct UMH {
    cfg: UMHConfig,
}

impl UMH {
    pub fn new(cfg: UMHConfig) -> Result<Self> {
        let mut cfg = cfg;
        for r in &mut cfg.resource {
            r.to_pattern();
        }
        for d in &mut cfg.device {
            d.to_pattern();
        }
        for pd in &mut cfg.peerdevice {
            pd.to_pattern();
        }
        for c in &mut cfg.connection {
            c.to_pattern();
        }

        Ok(Self { cfg })
    }
}

impl super::Plugin for UMH {
    fn run(&self, rx: super::PluginReceiver) -> Result<()> {
        trace!("umh: start");

        for r in rx.into_iter() {
            match r.as_ref() {
                // TODO(): this is quire repetitve, think about how to refactor.
                PluginUpdate::Resource(u) => {
                    for r in &self.cfg.resource {
                        if u.matches(&r.pattern) {
                            info!("umh: match for rule '{}'", r.common.name);
                            spawn_command(
                                r.common.command.clone(),
                                u.get_env(),
                                r.common.env.clone(),
                            );
                        }
                    }
                }
                PluginUpdate::Device(u) => {
                    for d in &self.cfg.device {
                        if u.matches(&d.pattern) {
                            info!("umh: match for rule '{}'", d.common.name);
                            spawn_command(
                                d.common.command.clone(),
                                u.get_env(),
                                d.common.env.clone(),
                            );
                        }
                    }
                }
                PluginUpdate::PeerDevice(u) => {
                    for pd in &self.cfg.peerdevice {
                        if u.matches(&pd.pattern) {
                            info!("umh: match for rule '{}'", pd.common.name);
                            spawn_command(
                                pd.common.command.clone(),
                                u.get_env(),
                                pd.common.env.clone(),
                            );
                        }
                    }
                }
                PluginUpdate::Connection(u) => {
                    for c in &self.cfg.connection {
                        if u.matches(&c.pattern) {
                            info!("umh: match for rule '{}'", c.common.name);
                            spawn_command(
                                c.common.command.clone(),
                                u.get_env(),
                                c.common.env.clone(),
                            );
                        }
                    }
                }
            }
        }

        trace!("umh: exit");
        Ok(())
    }
}

fn spawn_command(
    cmd: String,
    filter_env: HashMap<String, String>,
    user_env: HashMap<String, String>,
) {
    debug!("umh: starting handler '{}'", cmd);

    let common_env = common_env();
    thread::spawn(move || {
        match Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .env_clear()
            .envs(&filter_env)
            .envs(&user_env)
            .envs(&common_env)
            .status()
        {
            Ok(status) => {
                if !status.success() {
                    warn!("handler did not not exit successfully")
                }
                // report exit status back via drbdsetup and cockie
            }
            Err(e) => warn!("Could not execute handler: {}", e),
        }
    });
}

fn common_env() -> HashMap<String, String> {
    let mut env = HashMap::new();

    env.insert("HOME".to_string(), "/".to_string());
    env.insert("TERM".to_string(), "linux".to_string());
    env.insert(
        "PATH".to_string(),
        "/sbin:/usr/sbin:/bin:/usr/bin".to_string(),
    );

    env
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct UMHConfig {
    resource: Vec<ResourceRule>,
    device: Vec<DeviceRule>,
    peerdevice: Vec<PeerDeviceRule>,
    connection: Vec<ConnectionRule>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct CommonRule {
    command: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    env: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
struct DeviceRule {
    #[serde(flatten)]
    common: CommonRule,

    event_type: Option<BasicPattern<EventType>>,
    resource_name: Option<BasicPattern<String>>,
    volume: Option<BasicPattern<i32>>,
    old: Option<DeviceUpdateStatePattern>,
    new: Option<DeviceUpdateStatePattern>,

    // needs to be filled
    pattern: Option<DevicePluginUpdatePattern>,
}

impl DeviceRule {
    fn to_pattern(&mut self) {
        let pattern = DevicePluginUpdatePattern {
            event_type: self.event_type.clone(),
            resource_name: self.resource_name.clone(),
            volume: self.volume.clone(),
            old: self.old.clone(),
            new: self.new.clone(),
            resource: None, // intentionally filtered
        };
        self.pattern = Some(pattern);
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ResourceRule {
    #[serde(flatten)]
    common: CommonRule,

    pub event_type: Option<BasicPattern<EventType>>,
    pub resource_name: Option<BasicPattern<String>>,
    pub old: Option<ResourceUpdateStatePattern>,
    pub new: Option<ResourceUpdateStatePattern>,

    // needs to be filled
    pattern: Option<ResourcePluginUpdatePattern>,
}

impl ResourceRule {
    fn to_pattern(&mut self) {
        let pattern = ResourcePluginUpdatePattern {
            event_type: self.event_type.clone(),
            resource_name: self.resource_name.clone(),
            old: self.old.clone(),
            new: self.new.clone(),
            resource: None, // intentionally filtered
        };
        self.pattern = Some(pattern);
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct PeerDeviceRule {
    #[serde(flatten)]
    common: CommonRule,

    pub event_type: Option<BasicPattern<EventType>>,
    pub resource_name: Option<BasicPattern<String>>,
    pub volume: Option<BasicPattern<i32>>,
    pub peer_node_id: Option<BasicPattern<i32>>,
    pub old: Option<PeerDeviceUpdateStatePattern>,
    pub new: Option<PeerDeviceUpdateStatePattern>,

    // needs to be filled
    pattern: Option<PeerDevicePluginUpdatePattern>,
}

impl PeerDeviceRule {
    fn to_pattern(&mut self) {
        let pattern = PeerDevicePluginUpdatePattern {
            event_type: self.event_type.clone(),
            resource_name: self.resource_name.clone(),
            volume: self.volume,
            peer_node_id: self.peer_node_id,
            old: self.old.clone(),
            new: self.new.clone(),
            resource: None, // intentionally filtered
        };
        self.pattern = Some(pattern);
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ConnectionRule {
    #[serde(flatten)]
    common: CommonRule,

    pub event_type: Option<BasicPattern<EventType>>,
    pub resource_name: Option<BasicPattern<String>>,
    pub peer_node_id: Option<BasicPattern<i32>>,
    pub old: Option<ConnectionUpdateStatePattern>,
    pub new: Option<ConnectionUpdateStatePattern>,

    // needs to be filled
    pattern: Option<ConnectionPluginUpdatePattern>,
}

impl ConnectionRule {
    fn to_pattern(&mut self) {
        let pattern = ConnectionPluginUpdatePattern {
            event_type: self.event_type.clone(),
            resource_name: self.resource_name.clone(),
            peer_node_id: self.peer_node_id,
            old: self.old.clone(),
            new: self.new.clone(),
            resource: None, // intentionally filtered
        };
        self.pattern = Some(pattern);
    }
}
