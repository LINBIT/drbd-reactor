use crate::plugin::{debugger, promoter};
use anyhow::{Result, Context};
use serde::{Deserialize, Serialize, Deserializer, Serializer};
use std::fs::read_to_string;
use std::path::PathBuf;
use structopt::StructOpt;
use log::LevelFilter;
use stderrlog::Timestamp;
use std::str::FromStr;
use serde::de::Error;
use std::fmt::Display;

#[derive(Debug, StructOpt)]
struct CliOpt {
    #[structopt(short, long, parse(from_os_str), default_value = "/etc/drbdd.toml")]
    config: PathBuf,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigOpt {
    pub plugins: Vec<String>,
    #[serde(default)]
    pub promoter: promoter::PromoterOpt,
    #[serde(default)]
    pub debugger: debugger::DebuggerOpt,
    #[serde(default)]
    pub log: LogOpt,
}

pub fn from_args() -> Result<ConfigOpt> {
    let cli_opt = CliOpt::from_args();

    let content = read_to_string(&cli_opt.config)
        .with_context(|| format!("Could not read config file: {}", cli_opt.config.display()))?;
    let config: ConfigOpt = toml::from_str(&content)
        .with_context(|| format!("Could not parse config file content: {}", cli_opt.config.display()))?;

    Ok(config)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LogOpt {
    #[serde(default)]
    pub quiet: bool,
    #[serde(default = "default_level", serialize_with = "serialize_levelfilter", deserialize_with = "deserialize_from_str")]
    pub level: LevelFilter,
    #[serde(default = "default_timestamp", serialize_with = "serialize_timestamp", deserialize_with = "deserialize_from_str")]
    pub timestamps: Timestamp,
}

impl Default for LogOpt {
    fn default() -> Self {
        LogOpt {
            quiet: false,
            level: default_level(),
            timestamps: default_timestamp(),
        }
    }
}

fn default_level() -> LevelFilter {
    LevelFilter::Info
}

fn default_timestamp() -> Timestamp {
    Timestamp::Off
}

fn deserialize_from_str<'de, D, T>(de: D) -> Result<T, D::Error> where D: Deserializer<'de>, T: FromStr, T::Err: Display {
    let s: &str = Deserialize::deserialize(de)?;
    let val = s.parse().map_err(D::Error::custom)?;
    Ok(val)
}

fn serialize_levelfilter<S>(l: &LevelFilter, ser: S) -> Result<S::Ok, S::Error> where S: Serializer {
    ser.serialize_str(l.as_str())
}

fn serialize_timestamp<S>(t: &Timestamp, ser: S) -> Result<S::Ok, S::Error> where S: Serializer {
    let s = match t {
        &Timestamp::Off => "off",
        &Timestamp::Nanosecond => "ns",
        &Timestamp::Microsecond => "us",
        &Timestamp::Millisecond => "ms",
        &Timestamp::Second => "s",
    };
    ser.serialize_str(s)
}