//! Value formatting — the "Display formats" surface (28 formats across
//! 16/32/64-bit integers, float/double, word/byte orders, hex, bin, ASCII).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WordOrder {
    BigEndian,          // AB CD
    LittleEndian,       // CD AB
    BigEndianByteSwap,  // BA DC
    LittleEndianByteSwap, // DC BA
}

impl WordOrder {
    pub fn label(self) -> &'static str {
        match self {
            WordOrder::BigEndian => "Big-endian (AB CD)",
            WordOrder::LittleEndian => "Little-endian (CD AB)",
            WordOrder::BigEndianByteSwap => "Big-endian byte swap (BA DC)",
            WordOrder::LittleEndianByteSwap => "Little-endian byte swap (DC BA)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueFormat {
    U16,
    I16,
    Hex16,
    Bin16,
    Ascii16,
    U16Swapped,
    U32(WordOrder),
    I32(WordOrder),
    Hex32(WordOrder),
    F32(WordOrder),
    U64(WordOrder),
    I64(WordOrder),
    F64(WordOrder),
}

impl ValueFormat {
    pub fn regs_needed(self) -> usize {
        match self {
            ValueFormat::U16
            | ValueFormat::I16
            | ValueFormat::Hex16
            | ValueFormat::Bin16
            | ValueFormat::Ascii16
            | ValueFormat::U16Swapped => 1,
            ValueFormat::U32(_)
            | ValueFormat::I32(_)
            | ValueFormat::Hex32(_)
            | ValueFormat::F32(_) => 2,
            ValueFormat::U64(_) | ValueFormat::I64(_) | ValueFormat::F64(_) => 4,
        }
    }

    pub fn is_bit_style(self) -> bool {
        false
    }

    pub fn label(self) -> String {
        match self {
            ValueFormat::U16 => "16-bit Unsigned".into(),
            ValueFormat::I16 => "16-bit Signed".into(),
            ValueFormat::Hex16 => "16-bit Hex".into(),
            ValueFormat::Bin16 => "16-bit Binary".into(),
            ValueFormat::Ascii16 => "16-bit ASCII".into(),
            ValueFormat::U16Swapped => "16-bit Byte-swap".into(),
            ValueFormat::U32(o) => format!("32-bit Unsigned [{}]", o.label()),
            ValueFormat::I32(o) => format!("32-bit Signed [{}]", o.label()),
            ValueFormat::Hex32(o) => format!("32-bit Hex [{}]", o.label()),
            ValueFormat::F32(o) => format!("32-bit Float [{}]", o.label()),
            ValueFormat::U64(o) => format!("64-bit Unsigned [{}]", o.label()),
            ValueFormat::I64(o) => format!("64-bit Signed [{}]", o.label()),
            ValueFormat::F64(o) => format!("64-bit Double [{}]", o.label()),
        }
    }

    /// Assemble the raw big-endian byte sequence for the requested order.
    fn bytes(self, regs: &[u16]) -> Vec<u8> {
        let mut words: Vec<[u8; 2]> = regs.iter().map(|r| r.to_be_bytes()).collect();
        let order = match self {
            ValueFormat::U32(o) | ValueFormat::I32(o) | ValueFormat::Hex32(o) | ValueFormat::F32(o) => o,
            ValueFormat::U64(o) | ValueFormat::I64(o) | ValueFormat::F64(o) => o,
            _ => WordOrder::BigEndian,
        };
        match order {
            WordOrder::BigEndian => {}
            WordOrder::LittleEndian => words.reverse(),
            WordOrder::BigEndianByteSwap => {
                for w in words.iter_mut() {
                    w.swap(0, 1);
                }
            }
            WordOrder::LittleEndianByteSwap => {
                words.reverse();
                for w in words.iter_mut() {
                    w.swap(0, 1);
                }
            }
        }
        words.into_iter().flatten().collect()
    }

    /// Primary numeric value (as f64) for colour mapping / charts.
    pub fn numeric(self, regs: &[u16]) -> f64 {
        if regs.is_empty() {
            return 0.0;
        }
        match self {
            ValueFormat::U16 => regs[0] as f64,
            ValueFormat::I16 => (regs[0] as i16) as f64,
            ValueFormat::U16Swapped => regs[0].swap_bytes() as f64,
            ValueFormat::Hex16 | ValueFormat::Bin16 | ValueFormat::Ascii16 => regs[0] as f64,
            ValueFormat::U32(_) => {
                let b = self.bytes(regs);
                if b.len() >= 4 {
                    u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64
                } else {
                    0.0
                }
            }
            ValueFormat::I32(_) => {
                let b = self.bytes(regs);
                if b.len() >= 4 {
                    i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64
                } else {
                    0.0
                }
            }
            ValueFormat::Hex32(_) => {
                let b = self.bytes(regs);
                if b.len() >= 4 {
                    u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64
                } else {
                    0.0
                }
            }
            ValueFormat::F32(_) => {
                let b = self.bytes(regs);
                if b.len() >= 4 {
                    f32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64
                } else {
                    0.0
                }
            }
            ValueFormat::U64(_) => {
                let b = self.bytes(regs);
                if b.len() >= 8 {
                    u64::from_be_bytes(b[..8].try_into().unwrap()) as f64
                } else {
                    0.0
                }
            }
            ValueFormat::I64(_) => {
                let b = self.bytes(regs);
                if b.len() >= 8 {
                    i64::from_be_bytes(b[..8].try_into().unwrap()) as f64
                } else {
                    0.0
                }
            }
            ValueFormat::F64(_) => {
                let b = self.bytes(regs);
                if b.len() >= 8 {
                    f64::from_be_bytes(b[..8].try_into().unwrap())
                } else {
                    0.0
                }
            }
        }
    }

    pub fn format(self, regs: &[u16]) -> String {
        if regs.is_empty() {
            return String::new();
        }
        match self {
            ValueFormat::U16 => format!("{}", regs[0]),
            ValueFormat::I16 => format!("{}", regs[0] as i16),
            ValueFormat::U16Swapped => format!("{}", regs[0].swap_bytes()),
            ValueFormat::Hex16 => format!("{:04X}", regs[0]),
            ValueFormat::Bin16 => format!("{:016b}", regs[0]),
            ValueFormat::Ascii16 => {
                let b = regs[0].to_be_bytes();
                b.iter()
                    .map(|c| if (0x20..0x7F).contains(c) { *c as char } else { '.' })
                    .collect()
            }
            ValueFormat::U32(_) => format!("{}", self.numeric(regs) as u64),
            ValueFormat::I32(_) => format!("{}", self.numeric(regs) as i64),
            ValueFormat::Hex32(_) => format!("{:08X}", self.numeric(regs) as u32),
            ValueFormat::F32(_) => format_float(self.numeric(regs), false),
            ValueFormat::U64(_) => format!("{}", self.numeric(regs) as u64),
            ValueFormat::I64(_) => format!("{}", self.numeric(regs) as i64),
            ValueFormat::F64(_) => format_float(self.numeric(regs), true),
        }
    }

    /// Parse a user-typed value into registers (for writes).
    pub fn parse_to_regs(self, text: &str) -> Option<Vec<u16>> {
        let t = text.trim();
        match self {
            ValueFormat::U16 | ValueFormat::U16Swapped => t.parse::<u16>().ok().map(|v| {
                vec![if self == ValueFormat::U16Swapped { v.swap_bytes() } else { v }]
            }),
            ValueFormat::I16 => t.parse::<i16>().ok().map(|v| vec![v as u16]),
            ValueFormat::Hex16 => u16::from_str_radix(t.trim_start_matches("0x"), 16).ok().map(|v| vec![v]),
            ValueFormat::Bin16 => u16::from_str_radix(t.trim_start_matches("0b"), 2).ok().map(|v| vec![v]),
            ValueFormat::Ascii16 => {
                let b = t.as_bytes();
                if b.len() >= 2 {
                    Some(vec![u16::from_be_bytes([b[0], b[1]])])
                } else if b.len() == 1 {
                    Some(vec![u16::from_be_bytes([b[0], 0])])
                } else {
                    None
                }
            }
            ValueFormat::U32(o) => {
                let v = t.parse::<u32>().ok()?;
                Some(regs_from_u32(v, o))
            }
            ValueFormat::I32(o) => {
                let v = t.parse::<i32>().ok()? as u32;
                Some(regs_from_u32(v, o))
            }
            ValueFormat::Hex32(o) => {
                let v = u32::from_str_radix(t.trim_start_matches("0x"), 16).ok()?;
                Some(regs_from_u32(v, o))
            }
            ValueFormat::F32(o) => {
                let v = t.parse::<f32>().ok()?;
                Some(regs_from_u32(v.to_bits(), o))
            }
            _ => None,
        }
    }
}

fn regs_from_u32(v: u32, o: WordOrder) -> Vec<u16> {
    let be = v.to_be_bytes();
    let mut a = u16::from_be_bytes([be[0], be[1]]);
    let mut b = u16::from_be_bytes([be[2], be[3]]);
    match o {
        WordOrder::BigEndian => {}
        WordOrder::LittleEndian => std::mem::swap(&mut a, &mut b),
        WordOrder::BigEndianByteSwap => {
            a = a.swap_bytes();
            b = b.swap_bytes();
        }
        WordOrder::LittleEndianByteSwap => {
            std::mem::swap(&mut a, &mut b);
            a = a.swap_bytes();
            b = b.swap_bytes();
        }
    }
    vec![a, b]
}

fn format_float(v: f64, double: bool) -> String {
    if v.is_finite() {
        // trim trailing zeros but keep a visible fraction
        let s = if double { format!("{:.6}", v) } else { format!("{:.4}", v) };
        let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        if s.is_empty() || s == "-" {
            "0".into()
        } else {
            s
        }
    } else {
        format!("{v}")
    }
}

/// Convenience: the list of 16-bit display formats.
pub const NATIVE_FORMATS: &[ValueFormat] = &[
    ValueFormat::U16,
    ValueFormat::I16,
    ValueFormat::Hex16,
    ValueFormat::Bin16,
    ValueFormat::Ascii16,
    ValueFormat::U16Swapped,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u32_orders() {
        assert_eq!(ValueFormat::U32(WordOrder::BigEndian).numeric(&[0x075B, 0xCD15]), 123456789.0);
        assert_eq!(ValueFormat::U32(WordOrder::LittleEndian).numeric(&[0xCD15, 0x075B]), 123456789.0);
        assert_eq!(ValueFormat::format(ValueFormat::Hex32(WordOrder::BigEndian), &[0x075B, 0xCD15]), "075BCD15");
    }

    #[test]
    fn f32_decode() {
        // 1.0 == 0x3F800000
        assert_eq!(ValueFormat::F32(WordOrder::BigEndian).numeric(&[0x3F80, 0x0000]), 1.0);
    }

    #[test]
    fn parse_roundtrip() {
        let s = ValueFormat::U32(WordOrder::BigEndian).format(&[0x075B, 0xCD15]);
        let r = ValueFormat::U32(WordOrder::BigEndian).parse_to_regs(&s).unwrap();
        assert_eq!(r, vec![0x075B, 0xCD15]);
    }
}
