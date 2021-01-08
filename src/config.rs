use crate::plugin::{debugger, promoter};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs::read_to_string;
use std::path::PathBuf;
use structopt::StructOpt;

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

    let content = read_to_string(cli_opt.config)?;
    let mut config: ConfigOpt = toml::from_str(&content)?;

    // TODO(): probaly use enums
    config.log.level = match &config.log.level[..] {
        "error" | "warn" | "info" | "debug" | "trace" => config.log.level,
        _ => "info".to_string(),
    };
    config.log.timestamps = match &config.log.timestamps[..] {
        "sec" | "ms" | "us" | "ns" => config.log.timestamps,
        _ => "none".to_string(),
    };

    Ok(config)
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct LogOpt {
    #[serde(default)]
    pub quiet: bool,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub timestamps: String,
}
