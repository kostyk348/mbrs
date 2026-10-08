//! Project / workspace persistence (JSON — tinyxml2 in the original).
//! Contains the connection, poll groups, SCADA tiles and value names.

use crate::logging::LogConfig;
use crate::modbus::ConnConfig;
use crate::names::Names;
use crate::scada::Tile;
use crate::store::PollGroup;
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 2;

#[derive(Clone, Serialize, Deserialize)]
pub struct Project {
    #[serde(default = "one")]
    pub version: u32,
    pub conn: ConnConfig,
    pub groups: Vec<PollGroup>,
    #[serde(default)]
    pub tiles: Vec<Tile>,
    #[serde(default)]
    pub names: Names,
    #[serde(default)]
    pub log: LogConfig,
}

fn one() -> u32 {
    FORMAT_VERSION
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            conn: ConnConfig::default(),
            groups: vec![PollGroup::new(1)],
            tiles: vec![],
            names: Names::default(),
            log: LogConfig::default(),
        }
    }
}

impl Project {
    pub fn save(&self, path: &str) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    pub fn load(path: &str) -> Result<Self> {
        let data = std::fs::read_to_string(path)?;
        let mut p: Project = serde_json::from_str(&data)?;
        if p.version == 0 {
            p.version = FORMAT_VERSION;
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_workspace_parses() {
        let s = include_str!("../examples/demo.mbw");
        let p: Project = serde_json::from_str(s).expect("demo.mbw must parse");
        assert!(!p.groups.is_empty());
        assert_eq!(p.groups[0].fc, 0x03);
        assert!(!p.tiles.is_empty());
    }

    #[test]
    fn roundtrip() {
        let p = Project::default();
        let s = serde_json::to_string(&p).unwrap();
        let q: Project = serde_json::from_str(&s).unwrap();
        assert_eq!(q.groups.len(), p.groups.len());
    }
}
