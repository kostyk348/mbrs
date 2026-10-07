//! Project / workspace persistence (JSON — tinyxml2 in the original).
//! Contains the connection, poll groups, SCADA tiles and value names.

use crate::modbus::ConnConfig;
use crate::scada::Tile;
use crate::store::PollGroup;
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub conn: ConnConfig,
    pub groups: Vec<PollGroup>,
    pub tiles: Vec<Tile>,
    pub names: Vec<(u16, String)>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            conn: ConnConfig::default(),
            groups: vec![PollGroup::new(1)],
            tiles: vec![],
            names: vec![],
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
