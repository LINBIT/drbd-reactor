use anyhow::Result;
use log::{debug, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::process::{Command, Stdio};
use std::thread;

use crate::drbd::{
    ConnectionPluginUpdatePattern, ConnectionUpdateStatePattern, DevicePluginUpdatePattern,
    DeviceUpdateStatePattern, EventType, PeerDevicePluginUpdatePattern,
    PeerDeviceUpdateStatePattern, PluginUpdate, ResourcePluginUpdatePattern,
    ResourceUpdateStatePattern,
};
use crate::matchable::{BasicPattern, PartialMatchable};
use crate::plugin::PluginCfg;

pub struct UMH {
    resource_rules: Vec<(CommonRule, Option<ResourcePluginUpdatePattern>)>,
    device_rules: Vec<(CommonRule, Option<DevicePluginUpdatePattern>)>,
    peer_device_rules: Vec<(CommonRule, Option<PeerDevicePluginUpdatePattern>)>,
    connection_rules: Vec<(CommonRule, Option<ConnectionPluginUpdatePattern>)>,
    cfg: UMHConfig,
}

impl UMH {
    pub fn new(cfg: UMHConfig) -> Result<Self> {
        let cfg_clone = cfg.clone();
        Ok(Self {
            resource_rules: cfg.resource.into_iter().map(Into::into).collect(),
            device_rules: cfg.device.into_iter().map(Into::into).collect(),
            peer_device_rules: cfg.peerdevice.into_iter().map(Into::into).collect(),
            connection_rules: cfg.connection.into_iter().map(Into::into).collect(),
            cfg: cfg_clone,
        })
    }
}

impl super::Plugin for UMH {
    fn run(&self, rx: super::PluginReceiver) -> Result<()> {
        trace!("run: start");

        for r in rx.into_iter() {
            let handlers = match r.as_ref() {
                PluginUpdate::Resource(r) => get_handlers_by_pattern(r, &self.resource_rules),
                PluginUpdate::Device(d) => get_handlers_by_pattern(d, &self.device_rules),
                PluginUpdate::PeerDevice(p) => get_handlers_by_pattern(p, &self.peer_device_rules),
                PluginUpdate::Connection(c) => get_handlers_by_pattern(c, &self.connection_rules),
                _ => continue,
            };

            for handler in handlers {
                info!("run: match for rule: {}", handler.name);
                spawn_command(&handler.command, &r.get_env(), &handler.env)
            }
        }

        trace!("run: exit");
        Ok(())
    }

    fn get_config(&self) -> PluginCfg {
        PluginCfg::UMH(self.cfg.clone())
    }
}

/// Given a matchable item and a list of rules, return every rule that applies
fn get_handlers_by_pattern<'a, T>(
    item: &'a T,
    rules: &'a [(CommonRule, T::Pattern)],
) -> Box<dyn Iterator<Item = &'a CommonRule> + 'a>
where
    T: PartialMatchable,
{
    let iter = rules
        .iter()
        .filter(move |(_, p)| item.matches(p))
        .map(|(c, _)| c);

    Box::new(iter)
}

fn spawn_command(
    cmd: &str,
    filter_env: &HashMap<String, String>,
    user_env: &BTreeMap<String, String>,
) {
    debug!("spawn_command: starting handler '{}'", cmd);

    let common_env = common_env();

    let child = match Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .env_clear()
        .envs(filter_env)
        .envs(user_env)
        .envs(common_env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("spawn_command: could not execute handler: {}", e);
            return;
        }
    };
    thread::spawn(move || match child.wait_with_output() {
        Ok(output) => {
            if !output.status.success() {
                warn!("spawn_command: handler did not not exit successfully")
            }
            let out = std::str::from_utf8(&output.stdout).unwrap_or("<Could not convert stdout>");
            let err = std::str::from_utf8(&output.stderr).unwrap_or("<Could not convert stderr>");
            if !out.is_empty() || !err.is_empty() {
                debug!(
                    "spawn_command: handler stdout: '{}'; stderr: '{}'",
                    out, err
                );
            }
        }
        Err(e) => warn!("spawn_command: could not execute handler: {}", e),
    });
}

fn common_env() -> impl Iterator<Item = (&'static str, &'static str)> {
    [
        ("HOME", "/"),
        ("TERM", "Linux"),
        ("PATH", "/sbin:/usr/sbin:/bin:/usr/bin"),
    ]
    .iter()
    .map(ToOwned::to_owned)
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone, Default)]
#[serde(default)]
pub struct UMHConfig {
    resource: Vec<ResourceRule>,
    device: Vec<DeviceRule>,
    peerdevice: Vec<PeerDeviceRule>,
    connection: Vec<ConnectionRule>,
    pub id: Option<String>, // ! deprecated !
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
struct CommonRule {
    command: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
struct DeviceRule {
    #[serde(flatten)]
    common: CommonRule,

    event_type: Option<BasicPattern<EventType>>,
    resource_name: Option<BasicPattern<String>>,
    volume: Option<BasicPattern<i32>>,
    old: Option<DeviceUpdateStatePattern>,
    new: Option<DeviceUpdateStatePattern>,
}

impl From<DeviceRule> for (CommonRule, Option<DevicePluginUpdatePattern>) {
    fn from(val: DeviceRule) -> Self {
        (
            val.common,
            Some(DevicePluginUpdatePattern {
                event_type: val.event_type,
                resource_name: val.resource_name,
                volume: val.volume,
                old: val.old,
                new: val.new,
                resource: None,
            }),
        )
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ResourceRule {
    #[serde(flatten)]
    common: CommonRule,

    event_type: Option<BasicPattern<EventType>>,
    resource_name: Option<BasicPattern<String>>,
    old: Option<ResourceUpdateStatePattern>,
    new: Option<ResourceUpdateStatePattern>,
}

impl From<ResourceRule> for (CommonRule, Option<ResourcePluginUpdatePattern>) {
    fn from(val: ResourceRule) -> Self {
        (
            val.common,
            Some(ResourcePluginUpdatePattern {
                event_type: val.event_type,
                resource_name: val.resource_name,
                old: val.old,
                new: val.new,
                resource: None,
            }),
        )
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct PeerDeviceRule {
    #[serde(flatten)]
    common: CommonRule,

    event_type: Option<BasicPattern<EventType>>,
    resource_name: Option<BasicPattern<String>>,
    volume: Option<BasicPattern<i32>>,
    peer_node_id: Option<BasicPattern<i32>>,
    old: Option<PeerDeviceUpdateStatePattern>,
    new: Option<PeerDeviceUpdateStatePattern>,
}

impl From<PeerDeviceRule> for (CommonRule, Option<PeerDevicePluginUpdatePattern>) {
    fn from(val: PeerDeviceRule) -> Self {
        (
            val.common,
            Some(PeerDevicePluginUpdatePattern {
                event_type: val.event_type,
                resource_name: val.resource_name,
                volume: val.volume,
                peer_node_id: val.peer_node_id,
                old: val.old,
                new: val.new,
                resource: None,
            }),
        )
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ConnectionRule {
    #[serde(flatten)]
    common: CommonRule,

    event_type: Option<BasicPattern<EventType>>,
    resource_name: Option<BasicPattern<String>>,
    peer_node_id: Option<BasicPattern<i32>>,
    old: Option<ConnectionUpdateStatePattern>,
    new: Option<ConnectionUpdateStatePattern>,
}

impl From<ConnectionRule> for (CommonRule, Option<ConnectionPluginUpdatePattern>) {
    fn from(val: ConnectionRule) -> Self {
        (
            val.common,
            Some(ConnectionPluginUpdatePattern {
                event_type: val.event_type,
                resource_name: val.resource_name,
                peer_node_id: val.peer_node_id,
                old: val.old,
                new: val.new,
                resource: None,
            }),
        )
    }
}
