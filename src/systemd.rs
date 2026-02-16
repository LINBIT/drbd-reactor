use std::env;
use std::fmt;
use std::io::{Error, ErrorKind};
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::OnceLock;

use anyhow::Result;
use colored::Colorize;
use shell_words;

use crate::plugin;

pub const SYSTEMD_RUN_PREFIX: &str = "/run/systemd/system";

static NOTIFY_SOCKET_CELL: OnceLock<Option<PathBuf>> = OnceLock::new();
fn notify_socket() -> &'static Option<PathBuf> {
    NOTIFY_SOCKET_CELL.get_or_init(|| {
        let notify_socket = env::var_os("NOTIFY_SOCKET")?;
        // keep the original, but unset for children
        env::remove_var("NOTIFY_SOCKET");
        Some(notify_socket.into())
    })
}

pub fn notify(msg: &str) -> Result<()> {
    let socket = match notify_socket() {
        Some(s) => s,
        None => return Ok(()),
    };

    let sock = UnixDatagram::unbound()?;
    let msg_complete = format!("{msg}\n");
    if sock.send_to(msg_complete.as_bytes(), socket)? != msg_complete.len() {
        Err(anyhow::anyhow!(
            "systemd notify: could not completely write '{msg}' to '{}",
            socket.display()
        ))
    } else {
        Ok(())
    }
}

pub fn daemon_reload() -> Result<()> {
    plugin::map_status(
        Command::new("systemctl")
            .stdin(Stdio::null())
            .arg("daemon-reload")
            .status(),
    )
}

pub fn show_property(unit: &str, property: &str) -> Result<String> {
    let output = Command::new("systemctl")
        .stdin(Stdio::null())
        .arg("show")
        .arg(format!("--property={property}"))
        .arg(unit)
        .output()?;
    let output = std::str::from_utf8(&output.stdout)?;
    // split_once('=') would be more elegant, but we want to support old rustc (e.g., bullseye)
    let mut split = output.splitn(2, '=');
    match (split.next(), split.next()) {
        (Some(k), Some(v)) if k == property => Ok(v.trim().to_string()),
        (Some(_), Some(_)) => Err(anyhow::anyhow!("Property did not start with '{property}='")),
        _ => Err(anyhow::anyhow!("Could not get property '{property}'")),
    }
}

// most of that inspired by systemc/src/basic/unit-def.c
#[derive(PartialEq, Clone)]
pub enum UnitActiveState {
    Active,
    Reloading,
    Inactive,
    Failed,
    Activating,
    Deactivating,
    Maintenance,
}
impl serde::Serialize for UnitActiveState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
impl FromStr for UnitActiveState {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Error> {
        match input {
            "active" => Ok(Self::Active),
            "reloading" => Ok(Self::Reloading),
            "inactive" => Ok(Self::Inactive),
            "failed" => Ok(Self::Failed),
            "activating" => Ok(Self::Activating),
            "deactivating" => Ok(Self::Deactivating),
            "maintenance" => Ok(Self::Maintenance),
            _ => Err(Error::new(
                ErrorKind::InvalidData,
                "unknown systemd ActiveState",
            )),
        }
    }
}
impl fmt::Display for UnitActiveState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Reloading => write!(f, "reloading"),
            Self::Inactive => write!(f, "inactive"),
            Self::Failed => write!(f, "failed"),
            Self::Activating => write!(f, "activating"),
            Self::Deactivating => write!(f, "deactivating"),
            Self::Maintenance => write!(f, "maintenance"),
        }
    }
}
impl UnitActiveState {
    pub fn terminal(&self, _verbose: bool) -> Result<String> {
        Ok(match self {
            Self::Active => "●".bold().green().to_string(),
            Self::Reloading => "↻".bold().green().to_string(),
            Self::Inactive => "○".to_string(),
            Self::Failed => "×".bold().red().to_string(),
            Self::Activating => "●".bold().to_string(),
            Self::Deactivating => "●".bold().to_string(),
            Self::Maintenance => "○".to_string(),
        })
    }
}

pub fn is_active(unit: &str) -> Result<bool> {
    let prop = show_property(unit, "ActiveState")?;
    let state = UnitActiveState::from_str(&prop)?;
    Ok(state == UnitActiveState::Active)
}

pub fn escaped_ocf_parse_to_env(
    name: &str,
    vendor: &str,
    agent: &str,
    args: &str,
) -> Result<(String, Vec<String>)> {
    let args = shell_words::split(args)?;

    if args.is_empty() {
        anyhow::bail!("promoter::systemd_ocf: agent needs at least one argument (its name)")
    }

    let ra_name = &args[0];
    let ra_name = format!("{ra_name}_{name}");
    let escaped_ra_name = escape_name(&ra_name);
    let service_name = format!("ocf.rs@{escaped_ra_name}.service");
    let mut env = Vec::with_capacity(args.len() - 1);
    for item in &args[1..] {
        let mut split = item.splitn(2, '=');
        let add = match (split.next(), split.next()) {
            (Some(k), Some(v)) => format!("OCF_RESKEY_{k}={}", escape_env(v)),
            (Some(k), None) => format!("OCF_RESKEY_{k}="),
            _ => continue, // skip empty items
        };
        env.push(add)
    }

    env.push(format!(
        "AGENT=/usr/lib/ocf/resource.d/{}/{}",
        escape_env(vendor),
        escape_env(agent)
    ));

    Ok((service_name, env))
}

pub fn escaped_services_target(name: &str) -> String {
    format!("drbd-services@{}.target", escape_name(name))
}

pub fn escaped_services_target_dir(name: &str) -> PathBuf {
    Path::new(SYSTEMD_RUN_PREFIX).join(format!("{}.d", escaped_services_target(name)))
}

// inlined copy from https://crates.io/crates/libsystemd
// inlined because currently not packaged in Ubuntu Focal
pub fn escape_name(name: &str) -> String {
    if name.is_empty() {
        return "".to_string();
    }

    let parts: Vec<String> = name
        .bytes()
        .enumerate()
        .map(|(n, b)| escape_byte(b, n))
        .collect();
    parts.join("")
}

// inlined copy from https://crates.io/crates/libsystemd
// inlined because currently not packaged in Ubuntu Focal
fn escape_byte(b: u8, index: usize) -> String {
    let c = char::from(b);
    match c {
        '/' => '-'.to_string(),
        ':' | '_' | '0'..='9' | 'a'..='z' | 'A'..='Z' => c.to_string(),
        '.' if index > 0 => c.to_string(),
        _ => format!(r#"\x{b:02x}"#),
    }
}

// this is a relaxed version of escape_{name,byte}, for example we don't want '/' to be replaced
// this can be optimized to really just escape what is strictly needed, but IMO fine as is
fn escape_env(name: &str) -> String {
    if name.is_empty() {
        return "".to_string();
    }

    let parts: Vec<String> = name
        .bytes()
        .map(|b| {
            let c = char::from(b);
            match c {
                '.' | '/' | ':' | '_' | '0'..='9' | 'a'..='z' | 'A'..='Z' => c.to_string(),
                _ => format!(r#"\x{b:02x}"#),
            }
        })
        .collect();
    parts.join("")
}

#[test]
fn test_ocf_parse_to_env() {
    let (name, env) = escaped_ocf_parse_to_env(
        "res1",
        "vendor1",
        "agent1",
        "name1\nk1=v1 \nk2=\"with whitespace\" k3=with\\ different\\ whitespace foo empty='' pass='*pass/'",
    )
    .expect("should work");

    assert_eq!(name, "ocf.rs@name1_res1.service");
    assert_eq!(
        &env[..],
        &[
            "OCF_RESKEY_k1=v1",
            "OCF_RESKEY_k2=with\\x20whitespace",
            "OCF_RESKEY_k3=with\\x20different\\x20whitespace",
            "OCF_RESKEY_foo=",
            "OCF_RESKEY_empty=",
            "OCF_RESKEY_pass=\\x2apass/",
            "AGENT=/usr/lib/ocf/resource.d/vendor1/agent1"
        ]
    );

    // escaping
    let (name, _env) = escaped_ocf_parse_to_env("res-1", "vendor1", "agent1", "name-1 do not care")
        .expect("should work");

    assert_eq!(name, "ocf.rs@name\\x2d1_res\\x2d1.service");
}
