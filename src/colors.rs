//! Value -> colour engine.
//!
//! This is where mbrs deliberately goes far beyond Modbus Poll: the original
//! offers only Normal + Rule 1 + Rule 2 (i.e. ~3 visual states, one colour
//! each). Here you get:
//!   * an unbounded list of ordered rules (first match wins),
//!   * a 32-colour palette for one-click binding,
//!   * a *Discrete* mode that maps a value range onto exactly N distinct
//!     levels (N up to 32 by default) — "paint a cell in at least 32 colours",
//!   * a *Smooth* mode that maps the range onto a continuous ramp.

use serde::{Deserialize, Serialize};

/// 32 visually distinct colours (a tuned qualitative palette).
pub const PALETTE32: [[u8; 3]; 32] = [
    [31, 119, 180],
    [255, 127, 14],
    [44, 160, 44],
    [214, 39, 40],
    [148, 103, 189],
    [140, 86, 75],
    [227, 119, 194],
    [127, 127, 127],
    [188, 189, 34],
    [23, 190, 207],
    [174, 199, 232],
    [255, 187, 120],
    [152, 223, 138],
    [255, 152, 150],
    [197, 176, 213],
    [196, 156, 148],
    [247, 182, 210],
    [199, 199, 199],
    [219, 219, 141],
    [158, 218, 229],
    [178, 118, 2],
    [60, 80, 140],
    [90, 170, 60],
    [180, 20, 20],
    [120, 70, 160],
    [110, 60, 50],
    [200, 90, 170],
    [80, 80, 80],
    [150, 150, 10],
    [10, 160, 180],
    [240, 140, 10],
    [10, 120, 220],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Palette {
    Traffic, // green -> yellow -> red
    Rainbow, // blue -> cyan -> green -> yellow -> red
    Viridis, // dark purple -> teal -> green -> yellow
    Heat,    // black -> red -> orange -> white
    Cool,    // cyan -> blue -> magenta
    Greys,
}

impl Palette {
    pub fn label(self) -> &'static str {
        match self {
            Palette::Traffic => "Traffic (g→y→r)",
            Palette::Rainbow => "Rainbow",
            Palette::Viridis => "Viridis",
            Palette::Heat => "Heat",
            Palette::Cool => "Cool",
            Palette::Greys => "Greys",
        }
    }
    pub const ALL: &'static [Palette] = &[
        Palette::Traffic,
        Palette::Rainbow,
        Palette::Viridis,
        Palette::Heat,
        Palette::Cool,
        Palette::Greys,
    ];
}

fn lerp(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round().clamp(0.0, 255.0) as u8;
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

fn ramp(stops: &[[u8; 3]], t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    if stops.is_empty() {
        return [128, 128, 128];
    }
    if stops.len() == 1 {
        return stops[0];
    }
    let seg = t * (stops.len() - 1) as f64;
    let i = seg.floor() as usize;
    let i = i.min(stops.len() - 2);
    lerp(stops[i], stops[i + 1], seg - i as f64)
}

pub fn palette_color(p: Palette, t: f64) -> [u8; 3] {
    match p {
        Palette::Traffic => ramp(&[[0, 160, 0], [230, 200, 0], [200, 20, 20]], t),
        Palette::Rainbow => ramp(
            &[[40, 60, 200], [0, 190, 220], [40, 180, 40], [230, 220, 0], [210, 30, 30]],
            t,
        ),
        Palette::Viridis => ramp(&[[68, 1, 84], [59, 82, 139], [33, 145, 140], [94, 201, 98], [253, 231, 37]], t),
        Palette::Heat => ramp(&[[0, 0, 0], [150, 0, 0], [255, 90, 0], [255, 230, 120]], t),
        Palette::Cool => ramp(&[[0, 200, 210], [0, 80, 200], [180, 40, 200]], t),
        Palette::Greys => ramp(&[[30, 30, 30], [230, 230, 230]], t),
    }
}

/// Relative-luminance based readable foreground.
pub fn contrast(bg: [u8; 3]) -> [u8; 3] {
    let lum = 0.2126 * bg[0] as f64 + 0.7152 * bg[1] as f64 + 0.0722 * bg[2] as f64;
    if lum > 140.0 {
        [15, 15, 15]
    } else {
        [235, 235, 235]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorOp {
    NotUsed,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    /// true when ALL bits of `value` are set in the raw register.
    BitAll,
    /// true when ANY bit of `value` is set (classic Modbus Poll "and").
    BitAny,
    /// value <= v < value2
    Range,
    /// v is one of the listed values
    InSet,
}

impl ColorOp {
    pub fn label(self) -> &'static str {
        match self {
            ColorOp::NotUsed => "not used",
            ColorOp::Eq => "equal to",
            ColorOp::Ne => "not equal to",
            ColorOp::Gt => "greater than",
            ColorOp::Ge => "greater or equal",
            ColorOp::Lt => "less than",
            ColorOp::Le => "less or equal",
            ColorOp::BitAll => "has all bits",
            ColorOp::BitAny => "has any bit",
            ColorOp::Range => "in range",
            ColorOp::InSet => "one of",
        }
    }
    pub const ALL: &'static [ColorOp] = &[
        ColorOp::NotUsed,
        ColorOp::Eq,
        ColorOp::Ne,
        ColorOp::Gt,
        ColorOp::Ge,
        ColorOp::Lt,
        ColorOp::Le,
        ColorOp::BitAll,
        ColorOp::BitAny,
        ColorOp::Range,
        ColorOp::InSet,
    ];
}

fn hex3(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColorRule {
    pub op: ColorOp,
    pub value: i64,
    pub value2: i64,
    pub set: Vec<i64>,
    pub bg: [u8; 3],
    pub fg: [u8; 3],
    pub auto_fg: bool,
    pub label: String,
    pub enabled: bool,
}

impl ColorRule {
    pub fn new(op: ColorOp, value: i64, bg: [u8; 3]) -> Self {
        Self {
            op,
            value,
            value2: value,
            set: vec![],
            bg,
            fg: contrast(bg),
            auto_fg: true,
            label: String::new(),
            enabled: true,
        }
    }

    fn matches(&self, v: f64, raw: u64) -> bool {
        let vi = v.round() as i64;
        match self.op {
            ColorOp::NotUsed => false,
            ColorOp::Eq => vi == self.value,
            ColorOp::Ne => vi != self.value,
            ColorOp::Gt => v > self.value as f64,
            ColorOp::Ge => v >= self.value as f64,
            ColorOp::Lt => v < self.value as f64,
            ColorOp::Le => v <= self.value as f64,
            ColorOp::BitAll => (raw & self.value as u64) == self.value as u64 && self.value != 0,
            ColorOp::BitAny => (raw & self.value as u64) != 0,
            ColorOp::Range => v >= self.value as f64 && v < self.value2 as f64,
            ColorOp::InSet => self.set.contains(&vi),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorMode {
    /// Ordered rules; first match wins.
    Rules,
    /// Map [lo,hi] onto exactly `levels` distinct ramp colours.
    Discrete,
    /// Map [lo,hi] onto a continuous ramp.
    Smooth,
}

impl ColorMode {
    pub fn label(self) -> &'static str {
        match self {
            ColorMode::Rules => "Rules",
            ColorMode::Discrete => "Discrete steps",
            ColorMode::Smooth => "Smooth ramp",
        }
    }
    pub const ALL: &'static [ColorMode] = &[ColorMode::Rules, ColorMode::Discrete, ColorMode::Smooth];
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColorMap {
    pub mode: ColorMode,
    pub normal_bg: [u8; 3],
    pub normal_fg: [u8; 3],
    pub auto_fg: bool,
    pub rules: Vec<ColorRule>,
    pub ramp_lo: f64,
    pub ramp_hi: f64,
    pub levels: u8, // 2..=32 (default 32)
    pub palette: Palette,
}

impl Default for ColorMap {
    fn default() -> Self {
        Self {
            mode: ColorMode::Rules,
            normal_bg: [30, 32, 38],
            normal_fg: [220, 220, 220],
            auto_fg: true,
            rules: vec![],
            ramp_lo: 0.0,
            ramp_hi: 100.0,
            levels: 32,
            palette: Palette::Traffic,
        }
    }
}

impl ColorMap {
    /// Return (bg, fg) for a value. `raw` is the raw 16-bit register (for bit ops).
    pub fn color_for(&self, v: f64, raw: u64) -> ([u8; 3], [u8; 3]) {
        match self.mode {
            ColorMode::Rules => {
                for r in &self.rules {
                    if r.enabled && r.op != ColorOp::NotUsed && r.matches(v, raw) {
                        let fg = if r.auto_fg { contrast(r.bg) } else { r.fg };
                        return (r.bg, fg);
                    }
                }
                (self.normal_bg, self.normal_fg)
            }
            ColorMode::Discrete | ColorMode::Smooth => {
                let span = self.ramp_hi - self.ramp_lo;
                let mut t = if span.abs() < f64::EPSILON {
                    0.0
                } else {
                    (v - self.ramp_lo) / span
                };
                t = t.clamp(0.0, 1.0);
                let bg = if self.mode == ColorMode::Discrete {
                    let lev = self.levels.clamp(2, 32) as f64;
                    let idx = (t * (lev - 1.0)).round();
                    palette_color(self.palette, idx / (lev - 1.0))
                } else {
                    palette_color(self.palette, t)
                };
                (bg, contrast(bg))
            }
        }
    }

    pub fn swatch_hex(c: [u8; 3]) -> String {
        hex3(c)
    }
}

/// Interactive helper used by the UI: build an evenly spaced 32-colour set.
pub fn discrete32(lo: f64, hi: f64, palette: Palette) -> Vec<(f64, f64, [u8; 3])> {
    let n = 32usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / (n as f64 - 1.0);
        let v = lo + t * (hi - lo);
        out.push((t, v, palette_color(palette, t)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_first_match() {
        let mut cm = ColorMap::default();
        cm.rules.push(ColorRule::new(ColorOp::Lt, 0, [200, 0, 0]));
        cm.rules.push(ColorRule::new(ColorOp::Gt, 10, [0, 200, 0]));
        assert_eq!(cm.color_for(-1.0, 0).0, [200, 0, 0]);
        assert_eq!(cm.color_for(20.0, 0).0, [0, 200, 0]);
        assert_eq!(cm.color_for(5.0, 0).0, cm.normal_bg);
    }

    #[test]
    fn discrete_produces_32_distinct() {
        let cm = ColorMap {
            mode: ColorMode::Discrete,
            ramp_lo: 0.0,
            ramp_hi: 31.0,
            levels: 32,
            palette: Palette::Rainbow,
            ..Default::default()
        };
        let mut set = std::collections::HashSet::new();
        for i in 0..32 {
            set.insert(cm.color_for(i as f64, 0).0);
        }
        assert!(set.len() >= 32, "expected >=32 distinct colours, got {}", set.len());
    }

    #[test]
    fn bit_rules() {
        let all = ColorRule::new(ColorOp::BitAll, 0b0110, [1, 2, 3]);
        assert!(all.matches(0.0, 0b0110));
        assert!(!all.matches(0.0, 0b0010));
        let any = ColorRule::new(ColorOp::BitAny, 0b0110, [1, 2, 3]);
        assert!(any.matches(0.0, 0b0010));
        assert!(!any.matches(0.0, 0b1000));
    }
}
