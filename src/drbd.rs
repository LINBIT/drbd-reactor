use serde::Serialize;
use std::io::{Error, ErrorKind};
use std::str::FromStr;

common_matchable![Vec<Connection>, Vec<Device>];
make_matchable![
    #[derive(Default, Debug, Serialize, Clone, PartialEq)]
    pub struct Resource {
        pub name: String,
        pub role: Role,
        pub suspended: bool,
        #[serde(rename = "write-ordering")]
        pub write_ordering: String,
        pub may_promote: bool,
        pub promotion_score: i32,
        pub devices: Vec<Device>,
        pub connections: Vec<Connection>,
    },
    ResourcePattern
];

#[derive(Default, Debug, Serialize, Clone, PartialEq)]
pub struct Device {
    pub name: String,
    pub volume: i32,
    pub minor: i32,
    #[serde(rename = "disk-state")]
    pub disk_state: DiskState,
    pub client: bool,
    pub quorum: bool,
    pub size: u64,
    pub read: u64,
    pub written: u64,
    #[serde(rename = "al-writes")]
    pub al_writes: u64,
    #[serde(rename = "bm-writes")]
    pub bm_writes: u64,
    #[serde(rename = "upper-pending")]
    pub upper_pending: u64,
    #[serde(rename = "lower-pending")]
    pub lower_pending: u64,
    #[serde(rename = "al-suspended")]
    pub al_suspended: bool,
    pub blocked: bool,
}

#[derive(Default, Debug, Serialize, Clone, PartialEq)]
pub struct PeerDevice {
    pub name: String,
    pub volume: i32,
    #[serde(rename = "peer-node-id")]
    pub peer_node_id: i32,
    #[serde(rename = "replication-state")]
    pub replication_state: ReplicationState,
    #[serde(rename = "conn-name")]
    pub conn_name: String,
    #[serde(rename = "peer-disk-state")]
    pub peer_disk_state: DiskState,
    #[serde(rename = "peer-client")]
    pub peer_client: bool,
    #[serde(rename = "resync-suspendend")]
    pub resync_suspended: bool,
    pub received: u64,
    pub sent: u64,
    #[serde(rename = "out-of-sync")]
    pub out_of_sync: u64,
    pub pending: u64,
    pub unacked: u64,
    #[serde(rename = "has-sync-details")]
    pub has_sync_details: bool,
    #[serde(rename = "has-online-verify-details")]
    pub has_online_verify_details: bool,
}

#[derive(Default, Debug, Serialize, Clone, PartialEq)]
pub struct Connection {
    pub name: String,
    #[serde(rename = "peer-node-id")]
    pub peer_node_id: i32,
    #[serde(rename = "conn-name")]
    pub conn_name: String,
    pub connection: ConnectionState,
    #[serde(rename = "peer-role")]
    pub peer_role: Role,
    pub congested: bool,
    #[serde(rename = "ap-in-flight")]
    pub ap_in_flight: u64,
    #[serde(rename = "rs-in-flight")]
    pub rs_in_flight: u64,
    pub peerdevices: Vec<PeerDevice>,
}

make_matchable![
    #[derive(Serialize, Debug, PartialEq, Clone)]
    pub enum Role {
        Unknown,
        Primary,
        Secondary,
    }
];

// this could be extern enum_derive, but simple enough
impl FromStr for Role {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Error> {
        match input {
            "Unknown" => Ok(Self::Unknown),
            "Primary" => Ok(Self::Primary),
            "Secondary" => Ok(Self::Secondary),
            _ => Err(Error::new(ErrorKind::InvalidData, "unknow role state")),
        }
    }
}
impl Default for Role {
    fn default() -> Self {
        Self::Unknown
    }
}

make_matchable![
    #[derive(Serialize, Debug, Clone, PartialEq)]
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
    }
];

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
            _ => Err(Error::new(ErrorKind::InvalidData, "unknow disk state")),
        }
    }
}
impl Default for DiskState {
    fn default() -> Self {
        Self::DUnknown
    }
}

make_matchable![
    #[derive(Serialize, Debug, Clone, PartialEq)]
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
    }
];

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
                "unknow connection state",
            )),
        }
    }
}
impl Default for ConnectionState {
    fn default() -> Self {
        Self::StandAlone
    }
}

make_matchable![
    #[derive(Serialize, Debug, Clone, PartialEq)]
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
    }
];

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
                "unknow replication state",
            )),
        }
    }
}
impl Default for ReplicationState {
    fn default() -> Self {
        Self::Off
    }
}

make_matchable![
    #[derive(Debug, Clone, PartialEq)]
    pub struct ResourceUpdateState {
        pub role: Role,
        pub may_promote: bool,
        pub promotion_score: i32,
    },
    ResourceUpdateStatePattern
];

make_matchable![
    #[derive(Debug, Clone, Default, PartialEq)]
    pub struct DeviceUpdateState {
        pub disk_state: DiskState,
        pub client: bool,
        pub quorum: bool,
        pub size: u64,
    },
    DeviceUpdateStatePattern
];

make_matchable![
    #[derive(Debug, Clone, Default, PartialEq)]
    pub struct PeerDeviceUpdateState {
        pub replication_state: ReplicationState,
        pub peer_disk_state: DiskState,
        pub peer_client: bool,
        pub resync_suspended: bool,
    },
    PeerDeviceUpdateStatePattern
];

make_matchable![
    #[derive(Debug, Clone, Default, PartialEq)]
    pub struct ConnectionUpdateState {
        pub conn_name: String,
        pub connection: ConnectionState,
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
    Stop,
}

make_matchable![
    #[derive(Debug, Clone)]
    pub struct ResourcePluginUpdate {
        pub event_type: EventType,
        pub resource_name: String,
        pub old: ResourceUpdateState,
        pub new: ResourceUpdateState,
        pub resource: Resource,
    },
    ResourcePluginUpdatePattern
];

make_matchable![
    #[derive(Debug, Clone)]
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

make_matchable![
    #[derive(Debug, Clone)]
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

make_matchable![
    #[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub enum PluginUpdate {
    Resource(ResourcePluginUpdate),
    Device(DevicePluginUpdate),
    PeerDevice(PeerDevicePluginUpdate),
    Connection(ConnectionPluginUpdate),
}

impl PluginUpdate {
    pub fn has_name(&self, name: &str) -> bool {
        match self {
            Self::Resource(u) => u.resource_name == name,
            Self::Device(u) => u.resource_name == name,
            Self::PeerDevice(u) => u.resource_name == name,
            Self::Connection(u) => u.resource_name == name,
        }
    }

    pub fn has_type(&self, search: &EventType) -> bool {
        match self {
            Self::Resource(u) => u.event_type == *search,
            Self::Device(u) => u.event_type == *search,
            Self::PeerDevice(u) => u.event_type == *search,
            Self::Connection(u) => u.event_type == *search,
        }
    }

    pub fn get_name(&self) -> String {
        match self {
            Self::Resource(u) => u.resource_name.to_string(),
            Self::Device(u) => u.resource_name.to_string(),
            Self::PeerDevice(u) => u.resource_name.to_string(),
            Self::Connection(u) => u.resource_name.to_string(),
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

    fn get_device(&self, device: &Device) -> Option<&Device> {
        self.devices.iter().find(|c| c.volume == device.volume)
    }

    fn get_device_mut(&mut self, device: &Device) -> Option<&mut Device> {
        self.devices.iter_mut().find(|c| c.volume == device.volume)
    }

    pub fn update_device(&mut self, device: &Device) {
        match self.get_device_mut(device) {
            Some(existing) => *existing = device.clone(),
            None => self.devices.push(device.clone()),
        }
    }

    pub fn delete_device(&mut self, volume_id: i32) {
        self.devices.retain(|x| x.volume != volume_id)
    }

    pub fn update_or_delete_device(&mut self, et: &EventType, device: &Device) {
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

        match self.get_device(device) {
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

    pub fn get_connection(&self, conn: &Connection) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|c| c.peer_node_id == conn.peer_node_id)
    }

    pub fn get_connection_mut(&mut self, conn: &Connection) -> Option<&mut Connection> {
        self.connections
            .iter_mut()
            .find(|c| c.peer_node_id == conn.peer_node_id)
    }

    pub fn update_connection(&mut self, conn: &Connection) {
        match self.get_connection_mut(&conn) {
            Some(existing) => *existing = conn.clone(),
            None => self.connections.push(conn.clone()),
        }
    }

    pub fn update_or_delete_connection(&mut self, et: &EventType, conn: &Connection) {
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
            connection: conn.connection.clone(),
            peer_role: conn.peer_role.clone(),
        };

        match self.get_connection(conn) {
            Some(existing) => {
                let old = ConnectionUpdateState {
                    congested: existing.congested,
                    conn_name: existing.conn_name.clone(),
                    connection: existing.connection.clone(),
                    peer_role: existing.peer_role.clone(),
                };

                self.update_or_delete_connection(et, conn);
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

    pub fn get_connection_peerdevice(&self, peerdevice: &PeerDevice) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|c| c.peer_node_id == peerdevice.peer_node_id)
    }

    pub fn get_connection_peerdevice_mut(
        &mut self,
        peerdevice: &PeerDevice,
    ) -> Option<&mut Connection> {
        self.connections
            .iter_mut()
            .find(|c| c.peer_node_id == peerdevice.peer_node_id)
    }

    pub fn get_peerdevice(&self, peerdevice: &PeerDevice) -> Option<&PeerDevice> {
        match self.get_connection_peerdevice(peerdevice) {
            Some(conn) => conn
                .peerdevices
                .iter()
                .find(|c| c.volume == peerdevice.volume),
            None => None,
        }
    }

    pub fn get_peerdevice_mut(&mut self, peerdevice: &PeerDevice) -> Option<&mut PeerDevice> {
        match self.get_connection_peerdevice_mut(peerdevice) {
            Some(conn) => conn
                .peerdevices
                .iter_mut()
                .find(|c| c.volume == peerdevice.volume),
            None => None,
        }
    }

    pub fn update_peerdevice(&mut self, peerdevice: &PeerDevice) {
        match self.get_connection_peerdevice_mut(&peerdevice) {
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
        match self
            .connections
            .iter_mut()
            .find(|c| c.peer_node_id == peer_node_id)
        {
            Some(conn) => {
                conn.peerdevices
                    .retain(|x| x.volume != peerdevice_volume_id);
            }
            None => (),
        }
    }

    pub fn update_or_delete_peerdevice(&mut self, et: &EventType, peerdevice: &PeerDevice) {
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

        match self.get_peerdevice(peerdevice) {
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
}

make_matchable![
    #[derive(Debug, Clone, PartialEq)]
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
            _ => Err(Error::new(ErrorKind::InvalidData, "unknow event")),
        }
    }
}
