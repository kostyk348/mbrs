//! SCADA-style tile model. A tile is bound to one register (or register pair)
//! and rendered as a value box, lamp, bar or gauge using the colour engine —
//! so an operator never has to read raw register tables.

use crate::colors::ColorMap;
use crate::formats::ValueFormat;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TileKind {
    Value,
    Lamp,
    Bar,
    Gauge,
}

impl TileKind {
    pub fn label(self) -> &'static str {
        match self {
            TileKind::Value => "Value",
            TileKind::Lamp => "Lamp",
            TileKind::Bar => "Bar",
            TileKind::Gauge => "Gauge",
        }
    }
    pub const ALL: &'static [TileKind] = &[TileKind::Value, TileKind::Lamp, TileKind::Bar, TileKind::Gauge];
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tile {
    pub id: u32,
    pub kind: TileKind,
    pub unit: u8,
    pub fc: u8,
    pub addr: u16,
    pub format: ValueFormat,
    pub label: String,
    pub unit_text: String,
    /// Normalised position/size on the SCADA canvas (0..1 fractions).
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub lo: f64,
    pub hi: f64,
    pub color: ColorMap,
}

impl Tile {
    pub fn new(id: u32, addr: u16) -> Self {
        Self {
            id,
            kind: TileKind::Value,
            unit: 1,
            fc: 0x03,
            addr,
            format: ValueFormat::U16,
            label: format!("REG {}", addr),
            unit_text: String::new(),
            x: 0.02,
            y: 0.02,
            w: 0.22,
            h: 0.16,
            lo: 0.0,
            hi: 100.0,
            color: ColorMap::default(),
        }
    }
}

/// Auto-layout helper: arrange `n` tiles on a grid.
pub fn autolayout(tiles: &mut [Tile], cols: usize) {
    let cols = cols.max(1);
    let cw = 1.0f32 / cols as f32;
    let rows = (tiles.len() + cols - 1) / cols;
    let ch = 1.0f32 / rows.max(1) as f32;
    for (i, t) in tiles.iter_mut().enumerate() {
        let c = i % cols;
        let r = i / cols;
        t.x = c as f32 * cw + 0.005;
        t.y = r as f32 * ch + 0.005;
        t.w = cw - 0.01;
        t.h = ch - 0.01;
    }
}
