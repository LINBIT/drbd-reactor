use std::collections::HashMap;
use std::fmt;
use std::io::{Error, ErrorKind};
use std::process::{Command, Stdio};
use std::slice::Iter;
use std::str::FromStr;

use regex::Regex;
use serde::{Deserialize, Serialize};

common_matchable![Vec<Connection>, Vec<Device>];
make_matchable![
    #[derive(Default, Debug, Serialize, Clone, PartialEq, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Resource {
        pub name: String,
        pub role: Role,
        pub suspended: bool,
        pub write_ordering: String,
        pub force_io_failures: bool,
        pub may_promote: bool,
        pub promotion_score: i32,
        pub devices: Vec<Device>,
        pub connections: Vec<Connection>,
    },
    ResourcePattern
];

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct BackingDevice(pub Option<String>);

impl FromStr for BackingDevice {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Error> {
        match input {
            "none" => Ok(Self(None)),
            _ => Ok(Self(Some(input.to_string()))),
        }
    }
}
impl fmt::Display for BackingDevice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.0 {
            Some(bd) => write!(f, "{}", bd),
            None => write!(f, "none"),
        }
    }
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Device {
    pub name: String,
    pub volume: i32,
    pub minor: i32,
    pub disk_state: DiskState,
    pub backing_dev: BackingDevice,
    pub client: bool,
    pub quorum: bool,
    pub size: u64,
    pub read: u64,
    pub written: u64,
    pub al_writes: u64,
    pub bm_writes: u64,
    pub upper_pending: u64,
    pub lower_pending: u64,
    pub al_suspended: bool,
    pub blocked: String,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct PeerDevice {
    pub name: String,
    pub volume: i32,
    pub peer_node_id: i32,
    pub replication_state: ReplicationState,
    pub conn_name: String,
    pub peer_disk_state: DiskState,
    pub peer_client: bool,
    pub resync_suspended: bool,
    pub received: u64,
    pub sent: u64,
    pub out_of_sync: u64,
    pub pending: u64,
    pub unacked: u64,
    pub has_sync_details: bool,
    pub has_online_verify_details: bool,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Connection {
    pub name: String,
    pub peer_node_id: i32,
    pub conn_name: String,
    pub connection: ConnectionState,
    pub peer_role: Role,
    pub congested: bool,
    pub ap_in_flight: u64,
    pub rs_in_flight: u64,
    pub peerdevices: Vec<PeerDevice>,
    pub paths: Vec<Path>,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Path {
    pub name: String,
    pub peer_node_id: i32,
    pub conn_name: String,
    pub local: String,
    pub peer: String,
    pub established: bool,
}

make_matchable![
    #[derive(Serialize, Deserialize, Hash, Debug, PartialEq, Eq, Clone)]
    pub enum Role {
        Unknown,
        Primary,
        Secondary,
        // if you extend this enum, also extend iterator()
    }
];

impl Role {
    pub fn iterator() -> Iter<'static, Role> {
        static ROLES: [Role; 3] = [Role::Unknown, Role::Primary, Role::Secondary];
        ROLES.iter()
    }
}

// this could be extern enum_derive, but simple enough
impl FromStr for Role {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Error> {
        match input {
            "Unknown" => Ok(Self::Unknown),
            "Primary" => Ok(Self::Primary),
            "Secondary" => Ok(Self::Secondary),
            _ => Err(Error::new(ErrorKind::InvalidData, "unknown role state")),
        }
    }
}
impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Unknown => write!(f, "Unknown"),
            Self::Primary => write!(f, "Primary"),
            Self::Secondary => write!(f, "Secondary"),
        }
    }
}
impl Default for Role {
    fn default() -> Self {
        Self::Unknown
    }
}

make_matchable![
    #[derive(Serialize, Deserialize, Hash, Debug, Clone, PartialEq, Eq)]
    pub enum DiskState {
        Diskless,
        Attaching,
        Detaching,
        Failed,
        Negotiating,
        Inconsistent,
        Outdated,
        DUnknown,
        Consistent,
        UpToDate,
        // if you extend this enum, also extend iterator()
    }
];

impl DiskState {
    pub fn iterator() -> Iter<'static, DiskState> {
        static DISKSTATES: [DiskState; 10] = [
            DiskState::Diskless,
            DiskState::Attaching,
            DiskState::Detaching,
            DiskState::Failed,
            DiskState::Negotiating,
            DiskState::Inconsistent,
            DiskState::Outdated,
            DiskState::DUnknown,
            DiskState::Consistent,
            DiskState::UpToDate,
        ];
        DISKSTATES.iter()
    }
}

impl FromStr for DiskState {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Error> {
        match input {
            "Diskless" => Ok(Self::Diskless),
            "Attaching" => Ok(Self::Attaching),
            "Detaching" => Ok(Self::Detaching),
            "Failed" => Ok(Self::Failed),
            "Negotiating" => Ok(Self::Negotiating),
            "Inconsistent" => Ok(Self::Inconsistent),
            "Outdated" => Ok(Self::Outdated),
            "DUnknown" => Ok(Self::DUnknown),
            "Consistent" => Ok(Self::Consistent),
            "UpToDate" => Ok(Self::UpToDate),
            _ => Err(Error::new(ErrorKind::InvalidData, "unknown disk state")),
        }
    }
}
impl fmt::Display for DiskState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Diskless => write!(f, "Diskless"),
            Self::Attaching => write!(f, "Attaching"),
            Self::Detaching => write!(f, "Detaching"),
            Self::Failed => write!(f, "Failed"),
            Self::Negotiating => write!(f, "Negotiating"),
            Self::Inconsistent => write!(f, "Inconsistent"),
            Self::Outdated => write!(f, "Outdated"),
            Self::DUnknown => write!(f, "DUnknown"),
            Self::Consistent => write!(f, "Consistent"),
            Self::UpToDate => write!(f, "UpToDate"),
        }
    }
}
impl Default for DiskState {
    fn default() -> Self {
        Self::DUnknown
    }
}

make_matchable![
    #[derive(Serialize, Deserialize, Hash, Debug, Clone, PartialEq, Eq)]
    pub enum ConnectionState {
        StandAlone,
        Disconnecting,
        Unconnected,
        Timeout,
        BrokenPipe,
        NetworkFailure,
        ProtocolError,
        TearDown,
        Connecting,
        Connected,
        // if you extend this enum, also extend iterator()
    }
];

impl ConnectionState {
    pub fn iterator() -> Iter<'static, ConnectionState> {
        static CONNECTIONSTATES: [ConnectionState; 10] = [
            ConnectionState::StandAlone,
            ConnectionState::Disconnecting,
            ConnectionState::Unconnected,
            ConnectionState::Timeout,
            ConnectionState::BrokenPipe,
            ConnectionState::NetworkFailure,
            ConnectionState::ProtocolError,
            ConnectionState::TearDown,
            ConnectionState::Connecting,
            ConnectionState::Connected,
        ];
        CONNECTIONSTATES.iter()
    }
}

impl FromStr for ConnectionState {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Error> {
        match input {
            "StandAlone" => Ok(Self::StandAlone),
            "Disconnecting" => Ok(Self::Disconnecting),
            "Unconnected" => Ok(Self::Unconnected),
            "Timeout" => Ok(Self::Timeout),
            "BrokenPipe" => Ok(Self::BrokenPipe),
            "NetworkFailure" => Ok(Self::NetworkFailure),
            "ProtocolError" => Ok(Self::ProtocolError),
            "TearDown" => Ok(Self::TearDown),
            "Connecting" => Ok(Self::Connecting),
            "Connected" => Ok(Self::Connected),
            _ => Err(Error::new(
                ErrorKind::InvalidData,
                "unknown connection state",
            )),
        }
    }
}
impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::StandAlone => write!(f, "StandAlone"),
            Self::Disconnecting => write!(f, "Disconnecting"),
            Self::Unconnected => write!(f, "Unconnected"),
            Self::Timeout => write!(f, "Timeout"),
            Self::BrokenPipe => write!(f, "BrokenPipe"),
            Self::NetworkFailure => write!(f, "NetworkFailure"),
            Self::ProtocolError => write!(f, "ProtocolError"),
            Self::TearDown => write!(f, "TearDown"),
            Self::Connecting => write!(f, "Connecting"),
            Self::Connected => write!(f, "Connected"),
        }
    }
}
impl Default for ConnectionState {
    fn default() -> Self {
        Self::StandAlone
    }
}

make_matchable![
    #[derive(Serialize, Deserialize, Hash, Debug, Clone, PartialEq, Eq)]
    pub enum ReplicationState {
        Off,
        Established,
        StartingSyncS,
        StartingSyncT,
        WFBitMapS,
        WFBitMapT,
        WFSyncUUID,
        SyncSource,
        SyncTarget,
        VerifyS,
        VerifyT,
        PausedSyncS,
        PausedSyncT,
        Ahead,
        Behind,
        // if you extend this enum, also extend iterator()
    }
];

impl ReplicationState {
    pub fn iterator() -> Iter<'static, ReplicationState> {
        static STATES: [ReplicationState; 15] = [
            ReplicationState::Off,
            ReplicationState::Established,
            ReplicationState::StartingSyncS,
            ReplicationState::StartingSyncT,
            ReplicationState::WFBitMapS,
            ReplicationState::WFBitMapT,
            ReplicationState::WFSyncUUID,
            ReplicationState::SyncSource,
            ReplicationState::SyncTarget,
            ReplicationState::VerifyS,
            ReplicationState::VerifyT,
            ReplicationState::PausedSyncS,
            ReplicationState::PausedSyncT,
            ReplicationState::Ahead,
            ReplicationState::Behind,
        ];
        STATES.iter()
    }
}

impl FromStr for ReplicationState {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Error> {
        match input {
            "Off" => Ok(Self::Off),
            "Established" => Ok(Self::Established),
            "StartingSyncS" => Ok(Self::StartingSyncS),
            "StartingSyncT" => Ok(Self::StartingSyncT),
            "WFBitMapS" => Ok(Self::WFBitMapS),
            "WFBitMapT" => Ok(Self::WFBitMapT),
            "WFSyncUUID" => Ok(Self::WFSyncUUID),
            "SyncSource" => Ok(Self::SyncSource),
            "SyncTarget" => Ok(Self::SyncTarget),
            "VerifyS" => Ok(Self::VerifyS),
            "VerifyT" => Ok(Self::VerifyT),
            "PausedSyncS" => Ok(Self::PausedSyncS),
            "PausedSyncT" => Ok(Self::PausedSyncT),
            "Ahead" => Ok(Self::Ahead),
            "Behind" => Ok(Self::Behind),
            _ => Err(Error::new(
                ErrorKind::InvalidData,
                "unknown replication state",
            )),
        }
    }
}
impl fmt::Display for ReplicationState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Off => write!(f, "Off"),
            Self::Established => write!(f, "Established"),
            Self::StartingSyncS => write!(f, "StartingSyncS"),
            Self::StartingSyncT => write!(f, "StartingSyncT"),
            Self::WFBitMapS => write!(f, "WFBitMapS"),
            Self::WFBitMapT => write!(f, "WFBitMapT"),
            Self::WFSyncUUID => write!(f, "WFSyncUUID"),
            Self::SyncSource => write!(f, "SyncSource"),
            Self::SyncTarget => write!(f, "SyncTarget"),
            Self::VerifyS => write!(f, "VerifyS"),
            Self::VerifyT => write!(f, "VerifyT"),
            Self::PausedSyncS => write!(f, "PausedSyncS"),
            Self::PausedSyncT => write!(f, "PausedSyncT"),
            Self::Ahead => write!(f, "Ahead"),
            Self::Behind => write!(f, "Behind"),
        }
    }
}
impl Default for ReplicationState {
    fn default() -> Self {
        Self::Off
    }
}

make_matchable![
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct ResourceUpdateState {
        pub role: Role,
        pub may_promote: bool,
        pub promotion_score: i32,
    },
    ResourceUpdateStatePattern
];

make_matchable![
    #[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct DeviceUpdateState {
        pub disk_state: DiskState,
        pub client: bool,
        pub quorum: bool,
        pub size: u64,
    },
    DeviceUpdateStatePattern
];

make_matchable![
    #[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct PeerDeviceUpdateState {
        pub replication_state: ReplicationState,
        pub peer_disk_state: DiskState,
        pub peer_client: bool,
        pub resync_suspended: bool,
    },
    PeerDeviceUpdateStatePattern
];

make_matchable![
    #[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct ConnectionUpdateState {
        pub conn_name: String,
        pub connection_state: ConnectionState,
        pub peer_role: Role,
        pub congested: bool,
    },
    ConnectionUpdateStatePattern
];

#[derive(Debug, PartialEq)]
pub enum EventUpdate {
    Resource(EventType, Resource),
    Device(EventType, Device),
    PeerDevice(EventType, PeerDevice),
    Connection(EventType, Connection),
    Path(EventType, Path),
    Stop,
    Reload,
    Flush,
}

make_matchable![
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct ResourcePluginUpdate {
        pub event_type: EventType,
        pub resource_name: String,
        pub old: ResourceUpdateState,
        pub new: ResourceUpdateState,
        pub resource: Resource,
    },
    ResourcePluginUpdatePattern
];

impl ResourcePluginUpdate {
    pub fn get_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();

        env.insert("DRBD_RES_NAME".to_string(), self.resource_name.clone());
        env.insert("DRBD_OLD_ROLE".to_string(), self.old.role.to_string());
        env.insert("DRBD_NEW_ROLE".to_string(), self.new.role.to_string());
        env.insert(
            "DRBD_OLD_MAY_PROMOTE".to_string(),
            self.old.may_promote.to_string(),
        );
        env.insert(
            "DRBD_NEW_MAY_PROMOTE".to_string(),
            self.new.may_promote.to_string(),
        );

        env
    }
}

make_matchable![
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct DevicePluginUpdate {
        pub event_type: EventType,
        pub resource_name: String,
        pub volume: i32,
        pub old: DeviceUpdateState,
        pub new: DeviceUpdateState,
        pub resource: Resource,
    },
    DevicePluginUpdatePattern
];

impl DevicePluginUpdate {
    pub fn get_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();

        env.insert("DRBD_RES_NAME".to_string(), self.resource_name.clone());

        if let Some(device) = self.resource.get_device(self.volume) {
            env.insert("DRBD_MINOR".to_string(), device.minor.to_string());
            env.insert(
                format!("DRBD_MINOR_{}", self.volume),
                device.minor.to_string(),
            );
            env.insert(
                "DRBD_BACKING_DEV".to_string(),
                device.backing_dev.to_string(),
            );
            env.insert(
                format!("DRBD_BACKING_DEV_{}", self.volume),
                device.backing_dev.to_string(),
            );
        }
        env.insert("DRBD_VOLUME".to_string(), self.volume.to_string());

        env.insert(
            "DRBD_OLD_DISK_STATE".to_string(),
            self.old.disk_state.to_string(),
        );
        env.insert(
            "DRBD_NEW_DISK_STATE".to_string(),
            self.new.disk_state.to_string(),
        );
        env.insert("DRBD_OLD_CLIENT".to_string(), self.old.client.to_string());
        env.insert("DRBD_NEW_CLIENT".to_string(), self.new.client.to_string());
        env.insert("DRBD_OLD_QUORUM".to_string(), self.old.quorum.to_string());
        env.insert("DRBD_NEW_QUORUM".to_string(), self.new.quorum.to_string());

        env
    }
}

make_matchable![
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct PeerDevicePluginUpdate {
        pub event_type: EventType,
        pub resource_name: String,
        pub volume: i32,
        pub peer_node_id: i32,
        pub old: PeerDeviceUpdateState,
        pub new: PeerDeviceUpdateState,
        pub resource: Resource,
    },
    PeerDevicePluginUpdatePattern
];

impl PeerDevicePluginUpdate {
    pub fn get_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();

        env.insert("DRBD_RES_NAME".to_string(), self.resource_name.clone());

        if let Some(device) = self.resource.get_device(self.volume) {
            env.insert("DRBD_MINOR".to_string(), device.minor.to_string());
            env.insert(
                format!("DRBD_MINOR_{}", self.volume),
                device.minor.to_string(),
            );
            env.insert(
                "DRBD_BACKING_DEV".to_string(),
                device.backing_dev.to_string(),
            );
            env.insert(
                format!("DRBD_BACKING_DEV_{}", self.volume),
                device.backing_dev.to_string(),
            );
        }
        env.insert(
            "DRBD_PEER_NODE_ID".to_string(),
            self.peer_node_id.to_string(),
        );

        env.insert(
            "DRBD_OLD_PEER_REPLICATION_STATE".to_string(),
            self.old.replication_state.to_string(),
        );
        env.insert(
            "DRBD_NEW_PEER_REPLICATION_STATE".to_string(),
            self.new.replication_state.to_string(),
        );

        env.insert(
            "DRBD_OLD_PEER_DISK_STATE".to_string(),
            self.old.peer_disk_state.to_string(),
        );
        env.insert(
            "DRBD_NEW_PEER_DISK_STATE".to_string(),
            self.new.peer_disk_state.to_string(),
        );

        env.insert(
            "DRBD_OLD_PEER_CLIENT".to_string(),
            self.old.peer_client.to_string(),
        );
        env.insert(
            "DRBD_NEW_PEER_CLIENT".to_string(),
            self.new.peer_client.to_string(),
        );

        env.insert(
            "DRBD_OLD_PEER_RESYNC_SUSPENDED".to_string(),
            self.old.resync_suspended.to_string(),
        );
        env.insert(
            "DRBD_NEW_PEER_RESYNC_SUSPENDED".to_string(),
            self.new.resync_suspended.to_string(),
        );

        env
    }
}

make_matchable![
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct ConnectionPluginUpdate {
        pub event_type: EventType,
        pub resource_name: String,
        pub peer_node_id: i32,
        pub old: ConnectionUpdateState,
        pub new: ConnectionUpdateState,
        pub resource: Resource,
    },
    ConnectionPluginUpdatePattern
];

impl ConnectionPluginUpdate {
    pub fn get_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();

        env.insert("DRBD_RES_NAME".to_string(), self.resource_name.clone());
        env.insert(
            "DRBD_PEER_NODE_ID".to_string(),
            self.peer_node_id.to_string(),
        );
        env.insert(
            "DRBD_CSTATE".to_string(),
            self.new.connection_state.to_string(),
        );

        env.insert(
            "DRBD_OLD_CONN_NAME".to_string(),
            self.old.conn_name.to_string(),
        );
        env.insert(
            "DRBD_NEW_CONN_NAME".to_string(),
            self.new.conn_name.to_string(),
        );

        env.insert(
            "DRBD_OLD_CONN_STATE".to_string(),
            self.old.connection_state.to_string(),
        );
        env.insert(
            "DRBD_NEW_CONN_STATE".to_string(),
            self.new.connection_state.to_string(),
        );

        env.insert(
            "DRBD_OLD_PEER_ROLE".to_string(),
            self.old.peer_role.to_string(),
        );
        env.insert(
            "DRBD_NEW_PEER_ROLE".to_string(),
            self.new.peer_role.to_string(),
        );

        env
    }
}

#[derive(Debug, Clone)]
pub enum PluginUpdate {
    Resource(ResourcePluginUpdate),
    Device(DevicePluginUpdate),
    PeerDevice(PeerDevicePluginUpdate),
    Connection(ConnectionPluginUpdate),
    ResourceOnly(EventType, Resource),
}

impl PluginUpdate {
    pub fn has_name(&self, name: &str) -> bool {
        match self {
            Self::Resource(u) => u.resource_name == name,
            Self::Device(u) => u.resource_name == name,
            Self::PeerDevice(u) => u.resource_name == name,
            Self::Connection(u) => u.resource_name == name,
            Self::ResourceOnly(_, r) => r.name == name,
        }
    }

    pub fn has_type(&self, search: &EventType) -> bool {
        match self {
            Self::Resource(u) => u.event_type == *search,
            Self::Device(u) => u.event_type == *search,
            Self::PeerDevice(u) => u.event_type == *search,
            Self::Connection(u) => u.event_type == *search,
            Self::ResourceOnly(t, _) => *t == *search,
        }
    }

    pub fn get_name(&self) -> String {
        match self {
            Self::Resource(u) => u.resource_name.to_string(),
            Self::Device(u) => u.resource_name.to_string(),
            Self::PeerDevice(u) => u.resource_name.to_string(),
            Self::Connection(u) => u.resource_name.to_string(),
            Self::ResourceOnly(_, r) => r.name.to_string(),
        }
    }

    pub fn get_env(&self) -> HashMap<String, String> {
        match self {
            Self::Resource(u) => u.get_env(),
            Self::Device(u) => u.get_env(),
            Self::PeerDevice(u) => u.get_env(),
            Self::Connection(u) => u.get_env(),
            Self::ResourceOnly(_, _) => HashMap::new(),
        }
    }

    pub fn get_resource(&self) -> Resource {
        match self {
            Self::Resource(u) => u.resource.clone(),
            Self::Device(u) => u.resource.clone(),
            Self::PeerDevice(u) => u.resource.clone(),
            Self::Connection(u) => u.resource.clone(),
            Self::ResourceOnly(_, r) => r.clone(),
        }
    }
}

impl Resource {
    pub fn with_name(name: &str) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    // does not update the name, the name can not change as of now
    pub fn update(&mut self, r: &Resource) {
        self.role = r.role.clone();
        self.suspended = r.suspended;
        self.write_ordering = r.write_ordering.clone();
        self.may_promote = r.may_promote;
        self.promotion_score = r.promotion_score;
    }

    fn get_device(&self, volume_id: i32) -> Option<&Device> {
        self.devices.iter().find(|c| c.volume == volume_id)
    }

    fn get_device_mut(&mut self, volume_id: i32) -> Option<&mut Device> {
        self.devices.iter_mut().find(|c| c.volume == volume_id)
    }

    pub fn update_device(&mut self, device: &Device) {
        match self.get_device_mut(device.volume) {
            Some(existing) => *existing = device.clone(),
            None => self.devices.push(device.clone()),
        }
    }

    pub fn delete_device(&mut self, volume_id: i32) {
        self.devices.retain(|x| x.volume != volume_id)
    }

    fn update_or_delete_device(&mut self, et: &EventType, device: &Device) {
        if *et == EventType::Destroy {
            self.delete_device(device.volume);
        } else {
            self.update_device(device);
        }
    }

    pub fn get_device_update(&mut self, et: &EventType, device: &Device) -> Option<PluginUpdate> {
        let new = DeviceUpdateState {
            disk_state: device.disk_state.clone(),
            client: device.client,
            quorum: device.quorum,
            size: device.size,
        };

        match self.get_device(device.volume) {
            Some(existing) => {
                let old = DeviceUpdateState {
                    disk_state: existing.disk_state.clone(),
                    client: existing.client,
                    quorum: existing.quorum,
                    size: existing.size,
                };

                self.update_or_delete_device(et, device);
                if old == new && *et != EventType::Destroy {
                    return None;
                }

                Some(PluginUpdate::Device(DevicePluginUpdate {
                    event_type: et.clone(),
                    resource_name: self.name.clone(),
                    volume: device.volume,
                    old,
                    new,
                    resource: self.clone(),
                }))
            }

            None => {
                self.update_or_delete_device(et, device);
                if *et == EventType::Destroy {
                    return None;
                }

                Some(PluginUpdate::Device(DevicePluginUpdate {
                    event_type: et.clone(),
                    resource_name: self.name.clone(),
                    volume: device.volume,
                    old: DeviceUpdateState {
                        ..Default::default()
                    },
                    new,
                    resource: self.clone(),
                }))
            }
        }
    }

    pub fn get_connection(&self, peer_node_id: i32) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|c| c.peer_node_id == peer_node_id)
    }

    pub fn get_connection_mut(&mut self, peer_node_id: i32) -> Option<&mut Connection> {
        self.connections
            .iter_mut()
            .find(|c| c.peer_node_id == peer_node_id)
    }

    pub fn update_connection(&mut self, conn: &Connection) {
        match self.get_connection_mut(conn.peer_node_id) {
            Some(existing) => *existing = conn.clone(),
            None => self.connections.push(conn.clone()),
        }
    }

    fn update_or_delete_connection(&mut self, et: &EventType, conn: &Connection) {
        if *et == EventType::Destroy {
            self.delete_connection(conn.peer_node_id);
        } else {
            self.update_connection(conn);
        }
    }

    pub fn get_connection_update(
        &mut self,
        et: &EventType,
        conn: &Connection,
    ) -> Option<PluginUpdate> {
        let new = ConnectionUpdateState {
            congested: conn.congested,
            conn_name: conn.conn_name.clone(),
            connection_state: conn.connection.clone(),
            peer_role: conn.peer_role.clone(),
        };

        match self.get_connection(conn.peer_node_id) {
            Some(existing) => {
                let old = ConnectionUpdateState {
                    congested: existing.congested,
                    conn_name: existing.conn_name.clone(),
                    connection_state: existing.connection.clone(),
                    peer_role: existing.peer_role.clone(),
                };

                // existing connection and we know that the conn we get here
                // does not contain any peerdevices or paths
                // we want to preserve the existing pds/paths in the existing connection
                // conn is just an update for the rest of the struct fields.
                let mut conn = conn.clone();
                conn.peerdevices = existing.peerdevices.to_vec();
                conn.paths = existing.paths.to_vec();
                self.update_or_delete_connection(et, &conn);
                if old == new && *et != EventType::Destroy {
                    return None;
                }

                Some(PluginUpdate::Connection(ConnectionPluginUpdate {
                    event_type: et.clone(),
                    resource_name: self.name.clone(),
                    peer_node_id: conn.peer_node_id,
                    old,
                    new,
                    resource: self.clone(),
                }))
            }
            None => {
                self.update_or_delete_connection(et, conn);
                if *et == EventType::Destroy {
                    return None;
                }

                Some(PluginUpdate::Connection(ConnectionPluginUpdate {
                    event_type: et.clone(),
                    resource_name: self.name.clone(),
                    peer_node_id: conn.peer_node_id,
                    old: ConnectionUpdateState {
                        ..Default::default()
                    },
                    new,
                    resource: self.clone(),
                }))
            }
        }
    }

    pub fn delete_connection(&mut self, peer_node_id: i32) {
        self.connections.retain(|x| x.peer_node_id != peer_node_id)
    }

    pub fn get_peerdevice(&self, peer_node_id: i32, peer_volume_id: i32) -> Option<&PeerDevice> {
        match self.get_connection(peer_node_id) {
            Some(conn) => conn.peerdevices.iter().find(|c| c.volume == peer_volume_id),
            None => None,
        }
    }

    pub fn get_peerdevice_mut(
        &mut self,
        peer_node_id: i32,
        peer_volume_id: i32,
    ) -> Option<&mut PeerDevice> {
        match self.get_connection_mut(peer_node_id) {
            Some(conn) => conn
                .peerdevices
                .iter_mut()
                .find(|c| c.volume == peer_volume_id),
            None => None,
        }
    }

    pub fn update_peerdevice(&mut self, peerdevice: &PeerDevice) {
        match self.get_connection_mut(peerdevice.peer_node_id) {
            None => {
                let mut conn = Connection {
                    peer_node_id: peerdevice.peer_node_id,
                    ..Default::default()
                };

                conn.peerdevices.push(peerdevice.clone());
                self.connections.push(conn)
            }
            Some(conn) => {
                match conn
                    .peerdevices
                    .iter_mut()
                    .find(|c| c.volume == peerdevice.volume)
                {
                    Some(pd) => *pd = peerdevice.clone(),
                    None => conn.peerdevices.push(peerdevice.clone()),
                }
            }
        }
    }

    pub fn delete_peerdevice(&mut self, peer_node_id: i32, peerdevice_volume_id: i32) {
        if let Some(conn) = self.get_connection_mut(peer_node_id) {
            conn.peerdevices
                .retain(|x| x.volume != peerdevice_volume_id);
        }
    }

    fn update_or_delete_peerdevice(&mut self, et: &EventType, peerdevice: &PeerDevice) {
        if *et == EventType::Destroy {
            self.delete_peerdevice(peerdevice.peer_node_id, peerdevice.volume);
        } else {
            self.update_peerdevice(peerdevice);
        }
    }

    pub fn get_peerdevice_update(
        &mut self,
        et: &EventType,
        peerdevice: &PeerDevice,
    ) -> Option<PluginUpdate> {
        let new = PeerDeviceUpdateState {
            peer_client: peerdevice.peer_client,
            peer_disk_state: peerdevice.peer_disk_state.clone(),
            replication_state: peerdevice.replication_state.clone(),
            resync_suspended: peerdevice.resync_suspended,
        };

        match self.get_peerdevice(peerdevice.peer_node_id, peerdevice.volume) {
            Some(existing) => {
                let old = PeerDeviceUpdateState {
                    peer_client: existing.peer_client,
                    peer_disk_state: existing.peer_disk_state.clone(),
                    replication_state: existing.replication_state.clone(),
                    resync_suspended: existing.resync_suspended,
                };

                self.update_or_delete_peerdevice(et, peerdevice);
                if old == new && *et != EventType::Destroy {
                    return None;
                }

                Some(PluginUpdate::PeerDevice(PeerDevicePluginUpdate {
                    event_type: et.clone(),
                    resource_name: self.name.clone(),
                    volume: peerdevice.volume,
                    peer_node_id: peerdevice.peer_node_id,
                    old,
                    new,
                    resource: self.clone(),
                }))
            }
            None => {
                self.update_or_delete_peerdevice(et, peerdevice);
                if *et == EventType::Destroy {
                    return None;
                }

                Some(PluginUpdate::PeerDevice(PeerDevicePluginUpdate {
                    event_type: et.clone(),
                    resource_name: self.name.clone(),
                    volume: peerdevice.volume,
                    peer_node_id: peerdevice.peer_node_id,
                    old: PeerDeviceUpdateState {
                        ..Default::default()
                    },
                    new,
                    resource: self.clone(),
                }))
            }
        }
    }

    pub fn update_path(&mut self, path: &Path) {
        match self.get_connection_mut(path.peer_node_id) {
            None => {
                let mut conn = Connection {
                    peer_node_id: path.peer_node_id,
                    ..Default::default()
                };

                conn.paths.push(path.clone());
                self.connections.push(conn)
            }
            Some(conn) => {
                match conn
                    .paths
                    .iter_mut()
                    .find(|p| p.local == path.local && p.peer == path.peer)
                {
                    Some(pa) => *pa = path.clone(),
                    None => conn.paths.push(path.clone()),
                }
            }
        }
    }

    pub fn delete_path(&mut self, peer_node_id: i32, local: &str, peer: &str) {
        if let Some(conn) = self.get_connection_mut(peer_node_id) {
            conn.paths.retain(|x| {
                !(x.peer_node_id == peer_node_id && x.local == local && x.peer == peer)
            });
        }
    }

    fn update_or_delete_path(&mut self, et: &EventType, path: &Path) {
        if *et == EventType::Destroy {
            self.delete_path(path.peer_node_id, &path.local, &path.peer);
        } else {
            self.update_path(path);
        }
    }

    pub fn get_path_update(&mut self, et: &EventType, path: &Path) -> Option<PluginUpdate> {
        self.update_or_delete_path(et, path);
        None
    }

    pub fn get_resource_update(
        &mut self,
        et: &EventType,
        update: &Resource,
    ) -> Option<PluginUpdate> {
        let new = ResourceUpdateState {
            may_promote: update.may_promote,
            promotion_score: update.promotion_score,
            role: update.role.clone(),
        };

        let old = ResourceUpdateState {
            may_promote: self.may_promote,
            promotion_score: self.promotion_score,
            role: self.role.clone(),
        };

        if *et != EventType::Destroy {
            self.update(update);
        }
        if old == new && *et != EventType::Destroy {
            return None;
        }

        Some(PluginUpdate::Resource(ResourcePluginUpdate {
            event_type: et.clone(),
            resource_name: self.name.clone(),
            old,
            new,
            resource: self.clone(),
        }))
    }

    pub fn to_plugin_updates(&self) -> Vec<PluginUpdate> {
        let mut updates = vec![];
        let mut r = Resource::with_name(&self.name);

        // Announce that a named resource exists.
        updates.push(PluginUpdate::Resource(ResourcePluginUpdate {
            event_type: EventType::Exists,
            old: ResourceUpdateState {
                role: Role::Unknown,
                promotion_score: 0,
                may_promote: false,
            },
            new: ResourceUpdateState {
                role: Role::Unknown,
                promotion_score: 0,
                may_promote: false,
            },
            resource: r.clone(),
            resource_name: r.name.clone(),
        }));

        for d in &self.devices {
            if let Some(u) = r.get_device_update(&EventType::Exists, d) {
                updates.push(u);
            }
        }

        for c in &self.connections {
            if let Some(u) = r.get_connection_update(&EventType::Exists, c) {
                updates.push(u);
            }

            for p in &c.paths {
                if let Some(u) = r.get_path_update(&EventType::Exists, p) {
                    updates.push(u);
                }
            }

            for pd in &c.peerdevices {
                if let Some(u) = r.get_peerdevice_update(&EventType::Exists, pd) {
                    updates.push(u);
                }
            }
        }

        if let Some(u) = r.get_resource_update(&EventType::Change, self) {
            updates.push(u);
        }

        updates
    }
}

make_matchable![
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum EventType {
        Exists,
        Create,
        Destroy,
        Change,
    }
];

impl FromStr for EventType {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Error> {
        match input {
            "exists" => Ok(Self::Exists),
            "create" => Ok(Self::Create),
            "destroy" => Ok(Self::Destroy),
            "change" => Ok(Self::Change),
            _ => Err(Error::new(ErrorKind::InvalidData, "unknown event")),
        }
    }
}

#[derive(PartialOrd, PartialEq, Default)]
pub struct Version {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Default)]
pub struct DRBDVersion {
    pub kmod: Version,
    pub utils: Version,
}

pub fn get_drbd_versions() -> anyhow::Result<DRBDVersion> {
    let version = match Command::new("drbdadm")
        .stdin(Stdio::null())
        .arg("--version")
        .output()
    {
        Ok(x) => x,
        Err(e) => return Err(anyhow::anyhow!("failed running drbdadm --version: {}", e)),
    };

    if !version.status.success() {
        return Err(anyhow::anyhow!(
            "'drbdadm --version' not executed successfully, stdout: '{}', stderr: '{}'",
            String::from_utf8(version.stdout).unwrap_or("<Could not convert stdout>".to_string()),
            String::from_utf8(version.stderr).unwrap_or("<Could not convert stderr>".to_string())
        ));
    }

    let pattern = Regex::new(r"^DRBDADM_VERSION_CODE=0x([[:xdigit:]]+)$")?;
    let utils = split_version(pattern, version.stdout.clone())?;

    let pattern = Regex::new(r"^DRBD_KERNEL_VERSION_CODE=0x([[:xdigit:]]+)$")?;
    let kmod = split_version(pattern, version.stdout)?;

    Ok(DRBDVersion { kmod, utils })
}

fn split_version(pattern: regex::Regex, stdout: Vec<u8>) -> anyhow::Result<Version> {
    let version = String::from_utf8(stdout)?;
    let version = version
        .lines()
        .find_map(|line| pattern.captures(line))
        .ok_or(anyhow::anyhow!(
            "Could not determine version from pattern '{}'",
            pattern
        ))?;

    let version = u32::from_str_radix(&version[1], 16)?;

    let major = ((version >> 16) & 0xff) as u8;
    let minor = ((version >> 8) & 0xff) as u8;
    let patch = (version & 0xff) as u8;

    Ok(Version {
        major,
        minor,
        patch,
    })
}
