//! Value names and binary (bit) names — the "Value Names" / "Binary Names"
//! dialogs. Maps a 16-bit register value to text, and bit index to text.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Names {
    /// register value -> descriptive text
    pub value: BTreeMap<u16, String>,
    /// bit index (0..15) -> name
    pub bits: BTreeMap<u8, String>,
}

impl Names {
    pub fn text(&self, v: u16) -> Option<&String> {
        self.value.get(&v)
    }
    pub fn bit(&self, b: u8) -> Option<&String> {
        self.bits.get(&b)
    }

    /// Import the plain-text format used by Modbus Poll: `value=Text` lines.
    pub fn import_txt(&mut self, s: &str) {
        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                if let Ok(n) = k.trim().parse::<u16>() {
                    self.value.insert(n, v.trim().to_string());
                }
            }
        }
    }

    pub fn export_txt(&self) -> String {
        let mut out = String::new();
        for (k, v) in &self.value {
            out.push_str(&format!("{k}={v}\n"));
        }
        out
    }

    pub fn set_bit(&mut self, b: u8, name: String) {
        if name.trim().is_empty() {
            self.bits.remove(&b);
        } else {
            self.bits.insert(b.min(15), name);
        }
    }
}

/// Expand a u16 into per-bit names (used by the "show binary names" grid mode).
pub fn bit_names(n: &Names) -> Vec<(u8, String)> {
    (0..16u8).map(|b| (b, n.bit(b).cloned().unwrap_or_else(|| format!("bit {b}")))).collect()
}
