use std::fmt::Display;
use std::str::FromStr;

use anyhow::Result;
use log::LevelFilter;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde::de::Error;
use stderrlog::Timestamp;

use crate::plugin::{debugger, promoter};

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