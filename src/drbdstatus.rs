use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};

// just enough for DRBD resource options to run the checks on it we are interested in

#[derive(Serialize, Deserialize, Clone)]
pub struct Resource {
    pub resource: String,
    pub options: Options,
    pub connections: Vec<Connection>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Options {
    pub auto_promote: bool,
    pub quorum: String,
    pub on_no_quorum: String,
    pub on_suspended_primary_outdated: String,
    pub on_no_data_accessible: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Connection {
    // even if we expect the net options to be set globally, they are
    // "inherited" downwards to the individual connections
    pub net: Net,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Net {
    pub rr_conflict: String,
    pub fencing: String,
}

pub fn get(name: &str) -> Result<Resource> {
    let output = Command::new("drbdsetup")
        .stdin(Stdio::null())
        .arg("show")
        .arg("--show-defaults")
        .arg("--json")
        .arg(name)
        .output()?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "'drbdsetup show' not executed successfully"
        ));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let [resource]: [Resource; 1] = serde_json::from_str(&stdout)?;
    if resource.resource != name {
        return Err(anyhow::anyhow!(
            "res name to check ('{name}') and drbdsetup show output ('{}') did not match",
            resource.resource
        ));
    }
    Ok(resource)
}
