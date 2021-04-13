use std::path::PathBuf;

use log::LevelFilter;
use serde::{Deserialize, Serialize};

use crate::plugin;

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
pub struct LogConfig {
    #[serde(default = "default_level")]
    pub level: LevelFilter,
    pub file: Option<PathBuf>,
}

fn default_statistics() -> u64 {
    60
}

fn default_level() -> LevelFilter {
    LevelFilter::Info
}

fn default_log() -> Vec<LogConfig> {
    return vec![LogConfig {
        level: default_level(),
        file: None,
    }];
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
}
