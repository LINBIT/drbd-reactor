use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::{fmt, fs};

use anyhow::Result;
use log::LevelFilter;
use serde::de::Error;
use serde::{Deserialize, Serialize};

use crate::plugin;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    #[serde(default = "default_log")]
    pub log: Vec<LogConfig>,

    // seconds
    #[serde(default = "default_statistics")]
    pub statistics_poll_interval: u64,

    #[serde(default)]
    pub snippets: Option<PathBuf>,

    #[serde(flatten)]
    pub plugins: plugin::PluginConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LogConfig {
    #[serde(default = "default_level")]
    pub level: LevelFilter,
    pub file: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Hash, PartialEq, Eq, Debug, Clone)]
#[serde(untagged)]
pub enum LocalAddress {
    Explicit(SocketAddr),
    #[serde(
        serialize_with = "unspecified_ser",
        deserialize_with = "unspecified_de"
    )]
    Unspecified(u16),
}

fn unspecified_ser<S>(port: &u16, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&format!(":{}", port))
}

fn unspecified_de<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let str = String::deserialize(deserializer)?;
    if !str.starts_with(':') {
        return Err(Error::custom("expected ':' to start unspecified address"));
    }

    str[1..].parse().map_err(Error::custom)
}

impl ToSocketAddrs for LocalAddress {
    type Iter = std::vec::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
        match self {
            LocalAddress::Explicit(addr) => Ok(vec![*addr].into_iter()),
            LocalAddress::Unspecified(port) => Ok(vec![
                (Ipv6Addr::UNSPECIFIED, *port).into(),
                (Ipv4Addr::UNSPECIFIED, *port).into(),
            ]
            .into_iter()),
        }
    }
}

impl fmt::Display for LocalAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocalAddress::Explicit(socket) => socket.fmt(f),
            LocalAddress::Unspecified(port) => write!(f, ":{}", port),
        }
    }
}

impl Default for LocalAddress {
    fn default() -> Self {
        LocalAddress::Unspecified(0)
    }
}

fn default_statistics() -> u64 {
    60
}

fn default_level() -> LevelFilter {
    LevelFilter::Info
}

fn default_log() -> Vec<LogConfig> {
    vec![LogConfig {
        level: default_level(),
        file: None,
    }]
}

pub fn read_snippets(path: impl IntoIterator<Item = impl AsRef<Path>>) -> Result<String> {
    let mut s = "\n".to_string();
    for snippet in path {
        s.push_str(&fs::read_to_string(snippet)?);
        s.push('\n');
    }

    Ok(s)
}

pub fn files_with_extension_in(path: &PathBuf, extension: &str) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let extension = ".".to_owned() + extension;
    for entry in fs::read_dir(path)? {
        let path = match entry {
            Ok(e) => e.path(),
            _ => continue,
        };
        if !path.is_file() {
            continue;
        }
        let path_str = path.to_str().ok_or(anyhow::anyhow!(
            "Could not convert '{}' to str",
            path.display()
        ))?;
        if !path_str.ends_with(&extension) {
            continue;
        }
        files.push(path);
    }

    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use toml;

    use super::*;

    const EMPTY_CFG: &str = "";
    const OVERRIDE_LOG_CFG: &str = r#"[[log]]
    level = "trace"
    file = "/var/log/drbd-reactor.log"
    "#;
    const LOCAL_ADDRESS_IPV4: &str = "address = \"127.0.0.1:9999\"";
    const LOCAL_ADDRESS_IPV6: &str = "address = \"[::1]:9999\"";
    const LOCAL_ADDRESS_UNSPECIFIED: &str = "address = \":9999\"";
    const LOCAL_ADDRESS_ERR: &str = "address = \"::9999\"";

    #[test]
    fn test_default_cfg() {
        let cfg: Config = toml::from_str(EMPTY_CFG).expect("cfg must parse");
        assert_eq!(cfg.log.len(), 1);
        assert_eq!(cfg.log[0].level, default_level());
        assert_eq!(cfg.log[0].file, None);
    }

    #[test]
    fn test_override_log_cfg() {
        let cfg: Config = toml::from_str(OVERRIDE_LOG_CFG).expect("cfg must parse");
        assert_eq!(cfg.log.len(), 1);
        assert_eq!(cfg.log[0].level, LevelFilter::Trace);
        assert_eq!(
            cfg.log[0].file,
            Some(PathBuf::from("/var/log/drbd-reactor.log"))
        );
    }

    #[derive(Deserialize)]
    struct AddressTest {
        address: LocalAddress,
    }

    #[test]
    fn test_local_address_ipv4() {
        let addr: AddressTest = toml::from_str(LOCAL_ADDRESS_IPV4).expect("must parse");
        assert_eq!(
            addr.address,
            LocalAddress::Explicit((Ipv4Addr::LOCALHOST, 9999).into())
        )
    }

    #[test]
    fn test_local_address_ipv6() {
        let addr: AddressTest = toml::from_str(LOCAL_ADDRESS_IPV6).expect("must parse");
        assert_eq!(
            addr.address,
            LocalAddress::Explicit((Ipv6Addr::LOCALHOST, 9999).into())
        )
    }

    #[test]
    fn test_local_address_unspecified() {
        let addr: AddressTest = toml::from_str(LOCAL_ADDRESS_UNSPECIFIED).expect("must parse");
        assert_eq!(addr.address, LocalAddress::Unspecified(9999))
    }

    #[test]
    fn test_local_address_err() {
        let addr: Result<AddressTest, _> = toml::from_str(LOCAL_ADDRESS_ERR);
        assert!(addr.is_err());
    }
}
