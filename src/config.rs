use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use log::LevelFilter;
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

pub fn read_snippets(path: &Vec<PathBuf>) -> Result<String> {
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
