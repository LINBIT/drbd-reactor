use std::collections::{BTreeMap, HashMap};
use std::convert::TryFrom;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::ops::Bound;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time;

use agentx::encodings;
use agentx::pdu;
use anyhow::{Context, Result};
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};

use crate::drbd;
use crate::drbd::{DiskState, EventType, PluginUpdate, ReplicationState, Resource};
use crate::plugin::PluginCfg;

static TERMINATE: AtomicBool = AtomicBool::new(false);
const OIDPREFIX: [u32; 7] = [1, 3, 6, 1, 4, 1, 23302]; // enterprise + LINBIT

pub struct AgentX {
    cfg: AgentXConfig,
    metrics: Arc<Mutex<Metrics>>,
    stream: Arc<RwLock<TcpStream>>,
    thread_handle: Option<thread::JoinHandle<Result<()>>>,
}

impl AgentX {
    pub fn new(cfg: AgentXConfig) -> Result<Self> {
        let cache_max = time::Duration::from_secs(cfg.cache_max);
        let metrics = Arc::new(Mutex::new(Metrics::new(
            cache_max,
            time::Duration::from_secs(15),
            cfg.peer_states,
        )));

        debug!("new: connecting to snmp daemon on address {}", cfg.address);
        let stream = TcpStream::connect(&cfg.address).context(format!(
            "Failed to connect to snmp daemon on address {}",
            cfg.address
        ))?;
        let stream = Arc::new(RwLock::new(stream));

        debug!("new: starting agentx tcp handler");
        let thread_handle = {
            let stream_clone = stream.clone();
            let metrics_clone = metrics.clone();
            let cfg = cfg.clone();
            let agent_timeout = time::Duration::from_secs(cfg.agent_timeout);
            thread::spawn(move || {
                agentx_handler(stream_clone, &metrics_clone, &cfg.address, agent_timeout)
            })
        };

        Ok(AgentX {
            cfg,
            metrics,
            stream,
            thread_handle: Some(thread_handle),
        })
    }
}

impl super::Plugin for AgentX {
    fn run(&self, rx: super::PluginReceiver) -> Result<()> {
        trace!("run: start");
        for r in rx {
            match r.as_ref() {
                PluginUpdate::ResourceOnly(EventType::Exists, u)
                | PluginUpdate::ResourceOnly(EventType::Create, u)
                | PluginUpdate::ResourceOnly(EventType::Change, u) => match self.metrics.lock() {
                    Ok(mut m) => m.update(u),
                    Err(e) => {
                        error!("run: could not lock metrics: {}", e);
                        return Err(anyhow::anyhow!("Tried accessing a poisoned lock"));
                    }
                },
                PluginUpdate::ResourceOnly(EventType::Destroy, u) => match self.metrics.lock() {
                    Ok(mut m) => m.delete(&u.name),
                    Err(e) => {
                        error!("run: could not lock metrics: {}", e);
                        return Err(anyhow::anyhow!("Tried accessing a poisoned lock"));
                    }
                },
                _ => (),
            }
        }

        trace!("run: exit");

        Ok(())
    }

    fn get_config(&self) -> PluginCfg {
        PluginCfg::AgentX(self.cfg.clone())
    }
}

impl Drop for AgentX {
    fn drop(&mut self) {
        // if we would have a simple "while !TERMINATE {}" loop, we could run into this:
        // handler: looses connection, for whatever reason and is about to re-establish
        // handler: TERMINATE check successful -> continues
        // drop: shutdown + TERMINATE (order does not even matter)
        // handler: now esablishes connection and hangs in read
        //
        // => kill the socket in a loop, and let the handler ack the termination
        TERMINATE.store(true, Ordering::Relaxed);
        {
            loop {
                {
                    let s = self.stream.read().unwrap();
                    let _ = s.shutdown(Shutdown::Both);
                }
                if !TERMINATE.load(Ordering::Relaxed) {
                    // handler reset it
                    break;
                } else {
                    // give it some more time, guess we can be aggressive here
                    thread::sleep(time::Duration::from_millis(200));
                }
            }
        }

        if let Some(handle) = self.thread_handle.take() {
            trace!("drop: wait for agentx_handler thread to shut down");
            let res = handle.join();
            trace!("drop: agentx_handler thread shut down {:?}", res);
        }
    }
}

fn agentx_handler_process_loop(
    stream: &Arc<RwLock<TcpStream>>,
    metrics: &Arc<Mutex<Metrics>>,
    agent_timeout: time::Duration,
) -> Result<()> {
    let agent_id = encodings::ID::try_from(OIDPREFIX.to_vec()).expect("OID prefix is valid");
    // create session
    debug!("agentx_handler_process_loop: create session");
    let mut open = pdu::Open::new(agent_id.clone(), "DRBD by drbd-reactor::agentx");
    open.timeout = agent_timeout;
    let bytes = open.to_bytes().expect("Open PDU can be converted to bytes");
    let resp = txrx(stream, &bytes)?;
    let session_id = resp.header.session_id;

    // register agent
    debug!("agentx_handler_process_loop: register agent");
    let mut register = pdu::Register::new(agent_id);
    register.header.session_id = session_id;
    let bytes = register
        .to_bytes()
        .expect("Register PDU can be converted to bytes");
    txrx(stream, &bytes)?;

    // main processing loop
    info!("agentx_handler_process_loop: processing agentx messages");
    loop {
        let (ty, bytes) = rx(stream)?;
        trace!("agentx_handler_process_loop:main: got request '{:?}'", ty);

        // net-snmpd the defacto standard unfortunately does not implement GetBulk for agentx
        // snmpbulk* still helps as it avoids all the "external" network back and forth, but even the bulk variants then degenerate to agentx GetNext
        let mut resp = match ty {
            pdu::Type::Get => get(&bytes, metrics)?,
            pdu::Type::GetNext => get_next(&bytes, metrics)?,
            _ => {
                return Err(anyhow::anyhow!(
                    "agentx_handler: main: header.ty={:?} unhandled",
                    ty
                ));
            }
        };
        let bytes = resp.to_bytes()?;
        tx(stream, &bytes)?;
    }
}

// this thread never tries to continue until the main thread told it to terminate
// for thread sync considerations please check AgentX::Drop
fn agentx_handler(
    stream: Arc<RwLock<TcpStream>>,
    metrics: &Arc<Mutex<Metrics>>,
    address: &str,
    agent_timeout: time::Duration,
) -> Result<()> {
    let mut initially_connected = true;

    loop {
        if TERMINATE.load(Ordering::Relaxed) {
            break;
        }
        if initially_connected {
            initially_connected = false;
        } else {
            // connection broke for a reason, give daemon some time...
            thread::sleep(time::Duration::from_secs(2));
            {
                let mut s = match stream.write() {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("agentx_handler: could not lock tcp stream: '{}'", e);
                        continue;
                    }
                };
                *s = match TcpStream::connect(address) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("agentx_handler: could not connect stream '{}'", e);
                        continue;
                    }
                };
            }
        }

        if let Err(e) = agentx_handler_process_loop(&stream, metrics, agent_timeout) {
            warn!("agentx_handler_process_loop: '{}'", e);
        }
    }

    // flag drop
    TERMINATE.store(false, Ordering::Relaxed);
    Ok(())
}

fn get(bytes: &Vec<u8>, metrics: &Arc<Mutex<Metrics>>) -> Result<pdu::Response> {
    let pkg = pdu::Get::from_bytes(bytes)?;
    trace!(
        "get: sid: {}, tid: {}",
        pkg.header.session_id,
        pkg.header.transaction_id
    );
    let mut resp = pdu::Response::from_header(&pkg.header);
    let vb = metrics
        .lock()
        .map_err(|_| anyhow::anyhow!("Tried accessing a poisoned lock"))?
        .get(&pkg.sr);
    trace!("get: vbs: {:?}", vb);
    resp.vb = Some(vb);

    Ok(resp)
}

fn get_next(bytes: &Vec<u8>, metrics: &Arc<Mutex<Metrics>>) -> Result<pdu::Response> {
    let pkg = pdu::GetNext::from_bytes(bytes)?;
    trace!(
        "getnext: sid: {}, tid: {}",
        pkg.header.session_id,
        pkg.header.transaction_id
    );
    let mut resp = pdu::Response::from_header(&pkg.header);
    let vb = metrics
        .lock()
        .map_err(|_| anyhow::anyhow!("Tried accessing a poisoned lock"))?
        .get_next(&pkg.sr);
    trace!("getnext: vbs: {:?}", vb);
    resp.vb = Some(vb);

    Ok(resp)
}

// for administrative messages where we send stuff and get a response pdu
fn txrx(stream: &Arc<RwLock<TcpStream>>, bytes: &Vec<u8>) -> Result<pdu::Response> {
    tx(stream, bytes)?;
    let (_, buf) = rx(stream)?;
    Ok(pdu::Response::from_bytes(&buf)?)
}

fn tx(stream: &Arc<RwLock<TcpStream>>, bytes: &Vec<u8>) -> Result<()> {
    let lock = match stream.read() {
        Ok(l) => l,
        Err(_) => return Err(anyhow::anyhow!("txrx: could not lock stream")),
    };
    let mut s: &TcpStream = &lock;
    s.write_all(bytes)?;

    Ok(())
}

fn rx(stream: &Arc<RwLock<TcpStream>>) -> Result<(pdu::Type, Vec<u8>)> {
    let mut buf = vec![0u8; 20];

    // hold it till the end of the function, last s.read_exact() needs it anyways
    let lock = match stream.read() {
        Ok(s) => s,
        Err(_) => return Err(anyhow::anyhow!("rx: could not lock stream")),
    };
    let mut s: &TcpStream = &lock;
    s.read_exact(&mut buf)?;
    let header = pdu::Header::from_bytes(&buf)?;
    buf.resize(20 + header.payload_length as usize, 0);
    s.read_exact(&mut buf[20..])?;

    Ok((header.ty, buf))
}

struct Metrics {
    mib: BTreeMap<encodings::ID, encodings::Value>,
    resources: HashMap<String, Resource>,
    dirty: bool,
    cache_max: time::Duration, // how long do we keep the cache in general
    cache_last: time::Instant,
    burst_max: time::Duration, // how long do we keep the cache in case of "bursts" (i.e., GetNext) even if cache_max expired
    burst_last: time::Instant,
    peer_states: bool,
    drbd_version: drbd::DRBDVersion,
}

impl Metrics {
    fn new(cache_max: time::Duration, burst_max: time::Duration, peer_states: bool) -> Self {
        let now = time::Instant::now();
        let one_sec = time::Duration::from_secs(1);
        let drbd_version = drbd::get_drbd_versions().unwrap_or_default();
        Self {
            mib: BTreeMap::new(),
            resources: HashMap::new(),
            dirty: true,
            cache_max,
            cache_last: now - cache_max - one_sec,
            burst_max,
            burst_last: now - burst_max - one_sec,
            peer_states,
            drbd_version,
        }
    }

    fn update(&mut self, resource: &Resource) {
        self.dirty = true;
        self.resources
            .insert(resource.name.clone(), resource.clone());
    }

    fn delete(&mut self, resource_name: &str) {
        self.dirty = true;
        self.resources.remove(resource_name);
    }

    fn generate_mib(&mut self) {
        let now = time::Instant::now();
        let cache_expired = now - self.cache_last > self.cache_max;
        trace!("cache_expired: {}, dirty: {}", cache_expired, self.dirty);
        if !self.dirty || !cache_expired {
            debug!(
                "using cached MIB (cache_expired: {}, dirty: {})",
                cache_expired, self.dirty
            );
            return;
        }
        debug!("generating MIB");

        // with the caches in place I guess that is good enough
        // deleting mib branches could be improved by only marking them als deleted in self.resources
        // then we could delete them from self.mib and actually deleted them from self.resources
        // but that would require different data structrues in self.resources
        // even then we would need to updated all the other metrics, which basically means regenerating self.mib
        // I really don't think we hit any performace bottlenecks with our cache and the "burst cache". KISS
        self.mib.clear();

        let mut meta_prefix = OIDPREFIX.to_vec();
        meta_prefix.extend(&[1, 1]);
        let meta_prefix = meta_prefix;
        self.mib.insert(
            gen_id(&meta_prefix, &[1]),
            encodings::Value::OctetString(encodings::OctetString(
                self.drbd_version.kmod.to_string(),
            )),
        );
        self.mib.insert(
            gen_id(&meta_prefix, &[2]),
            encodings::Value::OctetString(encodings::OctetString(
                self.drbd_version.utils.to_string(),
            )),
        );

        let mut resource_prefix = OIDPREFIX.to_vec();
        resource_prefix.extend(&[1, 2, 1]);
        let resource_prefix = resource_prefix;

        for (name, resource) in &self.resources {
            let mut vol_to_minor = HashMap::new();

            for d in &resource.devices {
                vol_to_minor.insert(d.volume, d.minor);
                let minor = d.minor as u32;

                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::Minor as u32, minor]),
                    encodings::Value::Integer(d.minor),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::ResourceName as u32, minor]),
                    encodings::Value::OctetString(encodings::OctetString(name.to_string())),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::ResourceRole as u32, minor]),
                    encodings::Value::OctetString(encodings::OctetString(
                        resource.role.to_string(),
                    )),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::ResourceSuspended as u32, minor]),
                    encodings::Value::Integer(bool_to_truth(resource.suspended)),
                );
                self.mib.insert(
                    gen_id(
                        &resource_prefix,
                        &[MIB::ResourceWriteOrdering as u32, minor],
                    ),
                    encodings::Value::OctetString(encodings::OctetString(
                        resource.write_ordering.to_string(),
                    )),
                );
                self.mib.insert(
                    gen_id(
                        &resource_prefix,
                        &[MIB::ResourceForceIOFailures as u32, minor],
                    ),
                    encodings::Value::Integer(bool_to_truth(resource.force_io_failures)),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::ResourceMayPromote as u32, minor]),
                    encodings::Value::Integer(bool_to_truth(resource.may_promote)),
                );
                self.mib.insert(
                    gen_id(
                        &resource_prefix,
                        &[MIB::ResourcePromotionScore as u32, minor],
                    ),
                    encodings::Value::Integer(resource.promotion_score),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::Volume as u32, minor]),
                    encodings::Value::Integer(d.volume),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::DiskState as u32, minor]),
                    encodings::Value::OctetString(encodings::OctetString(d.disk_state.to_string())),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::BackingDev as u32, minor]),
                    encodings::Value::OctetString(encodings::OctetString(
                        d.backing_dev.to_string(),
                    )),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::Client as u32, minor]),
                    encodings::Value::Integer(bool_to_truth(d.client)),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::Quorum as u32, minor]),
                    encodings::Value::Integer(bool_to_truth(d.quorum)),
                );
                // in general tables can be sparse, snmptable handles that well
                // if some tools can not cope with it people will tell us
                // rather unlikely this fails anyways
                if let Ok(snmp_size) = drbd_size_to_snmp(d.size) {
                    self.mib.insert(
                        gen_id(&resource_prefix, &[MIB::Size as u32, minor]),
                        encodings::Value::Gauge32(snmp_size.size), // fine, mib type is Unsigned32
                    );
                    self.mib.insert(
                        gen_id(&resource_prefix, &[MIB::SizeUnits as u32, minor]),
                        encodings::Value::Gauge32(snmp_size.unit), // fine, mib type is Unsigned32
                    );
                }
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::Read as u32, minor]),
                    encodings::Value::Counter64(d.read),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::Written as u32, minor]),
                    encodings::Value::Counter64(d.written),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::AlWrites as u32, minor]),
                    encodings::Value::Counter64(d.al_writes),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::BmWrites as u32, minor]),
                    encodings::Value::Counter64(d.bm_writes),
                );
                // these are usually very small, we can cap these...
                let upper = u32::try_from(d.upper_pending).unwrap_or(u32::MAX);
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::UpperPending as u32, minor]),
                    encodings::Value::Gauge32(upper), // fine, mib type is Unsigned32
                );
                // these are usually very small, we can cap these...
                let lower = u32::try_from(d.lower_pending).unwrap_or(u32::MAX);
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::LowerPending as u32, minor]),
                    encodings::Value::Gauge32(lower), // fine, mib type is Unsigned32
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::AlSuspended as u32, minor]),
                    encodings::Value::Integer(bool_to_truth(d.al_suspended)),
                );
                self.mib.insert(
                    gen_id(&resource_prefix, &[MIB::Blocked as u32, minor]),
                    encodings::Value::OctetString(encodings::OctetString(d.blocked.to_string())),
                );
            }

            if self.peer_states {
                let mut pd_states = HashMap::new();
                // init 0 values
                for minor in vol_to_minor.values() {
                    let minor = *minor as u32;
                    for ds in DiskState::iterator() {
                        let id = MIB::from_disk_state(ds);
                        let id = gen_id(&resource_prefix, &[id as u32, minor]);
                        pd_states.insert(id, 0);
                    }
                    for rs in ReplicationState::iterator() {
                        let id = MIB::from_replication_state(rs);
                        let id = gen_id(&resource_prefix, &[id as u32, minor]);
                        pd_states.insert(id, 0);
                    }
                    let id = gen_id(&resource_prefix, &[MIB::PeerNumberOfPeers as u32, minor]);
                    pd_states.insert(id, 0);
                }
                for c in &resource.connections {
                    for pd in &c.peerdevices {
                        let minor = match vol_to_minor.get(&pd.volume) {
                            Some(m) => m,
                            None => {
                                error!("generate_mib: could not find minor for peer volume");
                                continue; // at least snmptable deals fine with values that don't exist and prints them as '?'. unlikely/impossible anyways
                            }
                        };
                        let minor = *minor as u32;

                        // disk state
                        let id = MIB::from_disk_state(&pd.peer_disk_state);
                        let id = gen_id(&resource_prefix, &[id as u32, minor]);
                        let count = pd_states.entry(id).or_insert(0);
                        *count += 1;

                        // repl state
                        let id = MIB::from_replication_state(&pd.replication_state);
                        let id = gen_id(&resource_prefix, &[id as u32, minor]);
                        let count = pd_states.entry(id).or_insert(0);
                        *count += 1;

                        // nr peers
                        let id = gen_id(&resource_prefix, &[MIB::PeerNumberOfPeers as u32, minor]);
                        let count = pd_states.entry(id).or_insert(0);
                        *count += 1;
                    }
                }
                for (id, count) in pd_states {
                    self.mib.insert(id, encodings::Value::Integer(count));
                }
            }
        }

        self.cache_last = now; // good enough I guess or should we use a new Instant::now()?
        self.dirty = false;
    }

    fn get(&mut self, sr: &encodings::SearchRangeList) -> encodings::VarBindList {
        self.generate_mib();
        let mut vbs = Vec::new();

        for s in sr {
            let name = s.start.clone();
            let value = match self.mib.get(&name) {
                Some(v) => v.clone(),
                None => encodings::Value::NoSuchObject,
            };
            vbs.push(encodings::VarBind::new(name, value));
        }

        encodings::VarBindList(vbs)
    }

    fn get_next(&mut self, sr: &encodings::SearchRangeList) -> encodings::VarBindList {
        let now = time::Instant::now();
        let burst_expired = now - self.burst_last > self.burst_max;
        trace!("burst_expired: {}", burst_expired);
        if burst_expired {
            self.generate_mib();
        }
        // rearm:
        self.burst_last = now;

        let mut vbs = Vec::new();

        let mut end_of_mibs = 0;
        for s in sr {
            trace!("get_next: s.start: {:?}", s.start);
            trace!("get_next: s.end: {:?}", s.end);

            let vb = if s.start.include == 0 {
                let iter = self
                    .mib
                    .range((Bound::Excluded(&s.start), Bound::Unbounded));
                generate_vb(iter, &s.start, &s.end)
            } else {
                let iter = self
                    .mib
                    .range((Bound::Included(&s.start), Bound::Unbounded));
                generate_vb(iter, &s.start, &s.end)
            };
            if vb.data == encodings::Value::EndOfMibView {
                end_of_mibs += 1;
            }
            vbs.push(vb);
        }

        if end_of_mibs == sr.len() {
            self.burst_last = now - self.burst_max - time::Duration::from_secs(1);
        }

        encodings::VarBindList(vbs)
    }
}

fn gen_id(prefix: &Vec<u32>, extension: &[u32]) -> encodings::ID {
    let mut id = prefix.clone();
    id.extend(extension);

    encodings::ID::try_from(id).expect("ID can be constructed from a Vec<u32>")
}

enum MIB {
    Minor = 1,
    //
    ResourceName,
    ResourceRole,
    ResourceSuspended,
    ResourceWriteOrdering,
    ResourceForceIOFailures,
    ResourceMayPromote,
    ResourcePromotionScore,
    //
    Volume,
    DiskState,
    BackingDev,
    Client,
    Quorum,
    Size,
    SizeUnits,
    Read,
    Written,
    AlWrites,
    BmWrites,
    UpperPending,
    LowerPending,
    AlSuspended,
    Blocked,
    //
    PeerNumberOfPeers,
    PeerDiskDiskless,
    PeerDiskAttaching,
    PeerDiskDetaching,
    PeerDiskFailed,
    PeerDiskNegotiating,
    PeerDiskInconsistent,
    PeerDiskOutdated,
    PeerDiskUnknown,
    PeerDiskConsistent,
    PeerDiskUpToDate,
    //
    PeerReplOff,
    PeerReplEstablished,
    PeerReplStartingSyncS,
    PeerReplStartingSyncT,
    PeerReplWFBitMapS,
    PeerReplWFBitMapT,
    PeerReplWFSyncUUID,
    PeerReplSyncSource,
    PeerReplSyncTarget,
    PeerReplVerifyS,
    PeerReplVerifyT,
    PeerReplPausedSyncS,
    PeerReplPausedSyncT,
    PeerReplAhead,
    PeerReplBehind,
}

impl MIB {
    fn from_disk_state(d: &DiskState) -> Self {
        match d {
            DiskState::Diskless => MIB::PeerDiskDiskless,
            DiskState::Attaching => MIB::PeerDiskAttaching,
            DiskState::Detaching => MIB::PeerDiskDetaching,
            DiskState::Failed => MIB::PeerDiskFailed,
            DiskState::Negotiating => MIB::PeerDiskNegotiating,
            DiskState::Inconsistent => MIB::PeerDiskInconsistent,
            DiskState::Outdated => MIB::PeerDiskOutdated,
            DiskState::DUnknown => MIB::PeerDiskUnknown,
            DiskState::Consistent => MIB::PeerDiskConsistent,
            DiskState::UpToDate => MIB::PeerDiskUpToDate,
        }
    }
    fn from_replication_state(r: &ReplicationState) -> Self {
        match r {
            ReplicationState::Off => MIB::PeerReplOff,
            ReplicationState::Established => MIB::PeerReplEstablished,
            ReplicationState::StartingSyncS => MIB::PeerReplStartingSyncS,
            ReplicationState::StartingSyncT => MIB::PeerReplStartingSyncT,
            ReplicationState::WFBitMapS => MIB::PeerReplWFBitMapS,
            ReplicationState::WFBitMapT => MIB::PeerReplWFBitMapT,
            ReplicationState::WFSyncUUID => MIB::PeerReplWFSyncUUID,
            ReplicationState::SyncSource => MIB::PeerReplSyncSource,
            ReplicationState::SyncTarget => MIB::PeerReplSyncTarget,
            ReplicationState::VerifyS => MIB::PeerReplVerifyS,
            ReplicationState::VerifyT => MIB::PeerReplVerifyT,
            ReplicationState::PausedSyncS => MIB::PeerReplPausedSyncS,
            ReplicationState::PausedSyncT => MIB::PeerReplPausedSyncT,
            ReplicationState::Ahead => MIB::PeerReplAhead,
            ReplicationState::Behind => MIB::PeerReplBehind,
        }
    }
}

fn bool_to_truth(b: bool) -> i32 {
    match b {
        true => 1,
        false => 2,
    }
}

struct SNMPSize {
    size: u32,
    unit: u32, // in bytes
}

// this idea is from hrStorageTable which basically does the same
fn drbd_size_to_snmp(mut size: u64) -> Result<SNMPSize> {
    // drbd size is in KiB
    let mut unit: u32 = 1024;
    while size > u32::MAX as u64 {
        size /= 1024;
        unit = unit.checked_mul(1024).ok_or(anyhow::anyhow!(
            "Could not convert from DRBD size to SNMP size"
        ))?;
    }
    let size = size as u32;

    Ok(SNMPSize { size, unit })
}

fn generate_vb<'a, I>(vals: I, start: &encodings::ID, end: &encodings::ID) -> encodings::VarBind
where
    I: IntoIterator<Item = (&'a encodings::ID, &'a encodings::Value)>,
{
    // https://datatracker.ietf.org/doc/html/rfc2741#section-7.2.3.2
    let (id, value) = match vals.into_iter().next() {
        Some((k, _)) if !end.is_null() && k >= end => {
            (start.clone(), encodings::Value::EndOfMibView)
        }
        Some((k, v)) => (k.clone(), v.clone()),
        None => (start.clone(), encodings::Value::EndOfMibView),
    };

    encodings::VarBind::new(id, value)
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct AgentXConfig {
    #[serde(default = "default_address")]
    pub address: String,
    #[serde(default = "default_cache_max")]
    pub cache_max: u64,
    #[serde(default = "default_agent_timeout")]
    pub agent_timeout: u64,
    #[serde(default = "default_peer_states")]
    pub peer_states: bool,
}

fn default_address() -> String {
    "localhost:705".to_string()
}

fn default_cache_max() -> u64 {
    60
}

fn default_agent_timeout() -> u64 {
    60
}

fn default_peer_states() -> bool {
    true
}
