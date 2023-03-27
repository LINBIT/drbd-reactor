use std::collections::HashMap;
use std::fmt::Write;
use std::io::Read;
use std::io::Write as IOWrite;
use std::net::{TcpListener, TcpStream};
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result};
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};

use crate::drbd;
use crate::drbd::{ConnectionState, DiskState, EventType, PluginUpdate, Resource, Role};

pub struct Prometheus {
    cfg: PrometheusConfig,
    listener: TcpListener,
    thread_handle: Option<thread::JoinHandle<Result<()>>>,
    metrics: Arc<Mutex<Metrics>>,
}

impl Prometheus {
    pub fn new(cfg: PrometheusConfig) -> Result<Self> {
        let metrics = Arc::new(Mutex::new(Metrics::new(cfg.enums)));

        debug!("new: listening for connections on address {}", cfg.address);
        let listener = TcpListener::bind(&cfg.address)
            .context(format!("Failed to bind to {}", cfg.address))?;

        debug!("new: starting tcp listener");
        let thread_handle = {
            let listener_clone = listener.try_clone().context("failed to clone socket")?;
            let metrics_clone = metrics.clone();
            thread::spawn(move || tcp_handler(listener_clone, &metrics_clone))
        };

        Ok(Prometheus {
            cfg,
            listener,
            metrics,
            thread_handle: Some(thread_handle),
        })
    }
}

impl Drop for Prometheus {
    fn drop(&mut self) {
        unsafe {
            // This is safe: self.listener is a separate FD from the one used by the HTTP handler.
            // This means there is no chance for the FD to be already closed.
            libc::shutdown(self.listener.as_raw_fd(), libc::SHUT_RD);
        }

        if let Some(handle) = self.thread_handle.take() {
            trace!("drop: wait for server thread to shut down");
            let res = handle.join();
            trace!("drop: server thread shut down {:?}", res);
        }
    }
}

impl super::Plugin for Prometheus {
    fn run(&self, rx: super::PluginReceiver) -> Result<()> {
        trace!("run: start");
        for r in rx {
            match r.as_ref() {
                PluginUpdate::ResourceOnly(EventType::Exists, u)
                | PluginUpdate::ResourceOnly(EventType::Create, u)
                | PluginUpdate::ResourceOnly(EventType::Change, u) => match self.metrics.lock() {
                    Ok(mut m) => m.update(&u),
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

    fn get_id(&self) -> Option<String> {
        self.cfg.id.clone()
    }
}

fn tcp_handler(listener: TcpListener, metrics: &Arc<Mutex<Metrics>>) -> Result<()> {
    for stream in listener.incoming() {
        let stream = stream.context("closed socket")?;

        if let Err(e) = handle_connection(stream, metrics) {
            // warn but continue processing
            warn!("tcp_handler: could not handle connection: {}", e);
        }
    }

    Ok(())
}

fn handle_connection(mut stream: TcpStream, metrics: &Arc<Mutex<Metrics>>) -> Result<()> {
    // read request body
    // we have to, otherwise we will get a connection reset by peer
    let mut discard = [0u8; 4096];
    stream.read(&mut discard)?;

    let content = metrics
        .lock()
        .map_err(|_| anyhow::anyhow!("Tried accessing a poisoned lock"))?
        .get()?;
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain;version=0.0.4\r\nContent-Length: {}\r\n\r\n",
        content.len()
    );
    response.push_str(&content);

    stream.write_all(response.as_bytes())?;
    Ok(())
}

#[derive(Default)]
struct Metrics {
    resources: HashMap<String, Resource>,
    dirty: bool,
    cache: String,
    enums: bool,
    drbd_version: drbd::DRBDVersion,
}

impl Metrics {
    fn new(enums: bool) -> Self {
        let drbd_version = drbd::get_drbd_versions().unwrap_or_default();

        Self {
            resources: HashMap::new(),
            enums,
            dirty: true,
            drbd_version,
            ..Default::default()
        }
    }

    fn update(&mut self, resource: &Resource) {
        self.dirty = true;
        self.resources
            .insert(resource.name.clone(), resource.clone());
    }

    fn get(&mut self) -> Result<String> {
        if !self.dirty {
            trace!("Metrics::get: serving from cache");
            return Ok(self.cache.clone());
        }

        trace!("Metrics::get: calculating metrics");
        let mut metrics = HashMap::new();

        // higher level metric
        let (k, m) = type_gauge(
            "drbdreactor_up",
            "Boolean indicating whether or not drbdreactor is running. Always 1",
            &mut metrics,
        );
        write!(m, "{} 1\n", k)?;

        let (k, m) = type_gauge(
            "drbd_version",
            "Version of the loaded DRBD kernel module and DRBD utils",
            &mut metrics,
        );
        write!(
            m,
            "{}{{kmod=\"{}\",utils=\"{}\"}} 1\n",
            k, self.drbd_version.kmod, self.drbd_version.utils
        )?;

        let (k, m) = type_gauge(
            "drbd_resource_resources",
            "Number of resources",
            &mut metrics,
        );
        write!(m, "{} {}\n", k, self.resources.len())?;

        for (name, r) in &self.resources {
            if self.enums {
                let (k, m) = type_gauge(
                    "drbd_resource_role",
                    "DRBD role of the resource",
                    &mut metrics,
                );
                for role in Role::iterator() {
                    write!(
                        m,
                        "{}{{name=\"{}\",{}=\"{}\"}} {}\n",
                        k,
                        name,
                        k,
                        role,
                        (role == &r.role) as i32
                    )?;
                }
            }

            let (k, m) = type_gauge(
                "drbd_resource_suspended",
                "Boolean whether the resource is suspended",
                &mut metrics,
            );
            write!(m, "{}{{name=\"{}\"}} {}\n", k, name, r.suspended as i32)?;

            let (k, m) = type_gauge(
                "drbd_resource_maypromote",
                "Boolean whether the resource may be promoted to Primary",
                &mut metrics,
            );
            write!(m, "{}{{name=\"{}\"}} {}\n", k, name, r.may_promote as i32)?;

            let (k, m) = type_gauge(
                "drbd_resource_promotionscore",
                "The promotion score (higher is better) for the resource",
                &mut metrics,
            );
            write!(m, "{}{{name=\"{}\"}} {}\n", k, name, r.promotion_score)?;

            let (k, m) = type_gauge(
                "drbd_resource_forceiofailures",
                "Boolean whether the resource is configured (could be temporarily) to force IO failures (e.g., during secondary --force)",
                &mut metrics,
            );
            write!(
                m,
                "{}{{name=\"{}\"}} {}\n",
                k, name, r.force_io_failures as i32
            )?;

            // connection
            for c in &r.connections {
                let mut common = String::new();
                write!(common, "name=\"{}\"", name)?;
                write!(common, ",conn_name=\"{}\"", c.conn_name)?;
                write!(common, ",peer_node_id=\"{}\"", c.peer_node_id)?;
                // TODO(rck) write!(common, ",peer_ip=\"{}\"", c.XXX?;
                // TODO(rck) write!(common, ",peer_port=\"{}\"", c.XXX)?;

                if self.enums {
                    let (k, m) = type_gauge(
                        "drbd_connection_state",
                        "DRBD connection state",
                        &mut metrics,
                    );
                    for cstate in ConnectionState::iterator() {
                        write!(
                            m,
                            "{}{{{},{}=\"{}\"}} {}\n",
                            k,
                            common,
                            k,
                            cstate,
                            (cstate == &c.connection) as i32
                        )?;
                    }
                }

                for pd in &c.peerdevices {
                    let (k, m) = type_gauge("drbd_peerdevice_outofsync_bytes",
                                            "Number of bytes currently out of sync with this peer, according to the bitmap that DRBD has for it",
                                            &mut metrics);
                    write!(
                        m,
                        "{}{{{},volume=\"{}\"}} {}\n",
                        k,
                        common,
                        pd.volume,
                        pd.out_of_sync * 1024, // KiB
                    )?;
                }

                let (k, m) = type_gauge("drbd_connection_congested",
                                        "Boolean whether the TCP send buffer of the data connection is more than 80% filled",
                                        &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, c.congested as i32)?;

                let (k, m) = type_gauge(
                    "drbd_connection_apinflight_bytes",
                    "Number of application requests in flight (not completed)",
                    &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, c.ap_in_flight * 512)?; // 512 byte sectors

                let (k, m) = type_gauge(
                    "drbd_connection_rsinflight_bytes",
                    "Number of resync requests in flight",
                    &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, c.rs_in_flight * 512)?; // 512 byte sectors
            }

            for d in &r.devices {
                let mut common = String::new();
                write!(
                    common,
                    "name=\"{}\",volume=\"{}\",minor=\"{}\"",
                    name, d.volume, d.minor
                )?;
                if self.enums {
                    let (k, m) = type_gauge("drbd_device_state", "DRBD device state", &mut metrics);
                    for dstate in DiskState::iterator() {
                        write!(
                            m,
                            "{}{{{},{}=\"{}\"}} {}\n",
                            k,
                            common,
                            k,
                            dstate,
                            (dstate == &d.disk_state) as i32
                        )?;
                    }
                }

                let (k, m) = type_gauge(
                    "drbd_device_client",
                    "Boolean whether this device is a client (i.e., intentional diskless)",
                    &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, d.client as i32)?;

                // higher level metric
                let (k, m) = type_gauge(
                    "drbd_device_unintentionaldiskless",
                    "Boolean whether the devices is unintentional diskless",
                    &mut metrics,
                );
                write!(
                    m,
                    "{}{{{}}} {}\n",
                    k,
                    common,
                    (!d.client && d.disk_state == DiskState::Diskless) as i32
                )?;

                let (k, m) = type_gauge(
                    "drbd_device_quorum",
                    "Boolean if this device has DRBD quorum",
                    &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, d.quorum as i32)?;

                let (k, m) = type_gauge(
                    "drbd_device_size_bytes",
                    "Device size in bytes",
                    &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, d.size * 1024)?; // KiB

                let (k, m) = type_counter(
                    "drbd_device_read_bytes_total",
                    "Net data read from local hard disk",
                    &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, d.read * 1024)?; // KiB

                let (k, m) = type_counter(
                    "drbd_device_written_bytes_total",
                    "Net data written on local disk",
                    &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, d.written * 1024)?; // KiB

                let (k, m) = type_counter(
                    "drbd_device_alwrites_total",
                    "Number of updates of the activity log area of the meta data",
                    &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, d.al_writes)?;

                let (k, m) = type_counter(
                    "drbd_device_bmwrites_total",
                    "Number of updates of the bitmap area of the meta data",
                    &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, d.bm_writes)?;

                let (k, m) = type_gauge(
                    "drbd_device_upperpending",
                    "Number of block I/O requests forwarded to DRBD, but not yet answered by DRBD.",
                    &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, d.upper_pending)?;

                let (k, m) = type_gauge(
                    "drbd_device_lowerpending",
                    "Number of open requests to the local I/O sub-system issued by DRBD",
                    &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, d.lower_pending)?;

                let (k, m) = type_gauge(
                    "drbd_device_alsuspended",
                    "Boolean whether the Activity-Log is suspended",
                    &mut metrics,
                );
                write!(m, "{}{{{}}} {}\n", k, common, d.al_suspended as i32)?;
            }
        }

        self.cache.clear();
        metrics.values().for_each(|v| self.cache.push_str(&v));
        self.dirty = false;
        Ok(self.cache.clone())
    }

    fn delete(&mut self, resource_name: &str) {
        self.dirty = true;
        self.resources.remove(resource_name);
    }
}

fn header_generic(k: &str, help: &str, mtype: &str) -> (String, String) {
    (
        k.to_string(),
        format!("# TYPE {} {}\n# HELP {}\n", k, mtype, help),
    )
}

fn header_gauge(k: &str, help: &str) -> (String, String) {
    header_generic(k, help, "gauge")
}

fn header_counter(k: &str, help: &str) -> (String, String) {
    header_generic(k, help, "counter")
}

fn type_gauge<'a>(
    k: &'a str,
    help: &'a str,
    metrics: &'a mut HashMap<String, String>,
) -> (String, &'a mut String) {
    let (k, t) = header_gauge(k, help);
    let m = metrics.entry(k.clone()).or_insert(t);
    (k, m)
}

fn type_counter<'a>(
    k: &'a str,
    help: &'a str,
    metrics: &'a mut HashMap<String, String>,
) -> (String, &'a mut String) {
    let (k, t) = header_counter(k, help);
    let m = metrics.entry(k.clone()).or_insert(t);
    (k, m)
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct PrometheusConfig {
    #[serde(default = "default_address")]
    pub address: String,
    #[serde(default)]
    pub enums: bool,
    pub id: Option<String>,
}

fn default_address() -> String {
    "[::]:9942".to_string()
}
