//! Modbus codec + transport, implemented from scratch.
//!
//! Supported framings: Modbus/TCP (MBAP), Modbus/UDP, Modbus RTU (serial),
//! Modbus ASCII (serial / over TCP) and RTU/ASCII encapsulated in TCP.
//!
//! Supported function codes: 01,02,03,04,05,06,08,0B,0F,10,11,16,17,22,23,43/14.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpStream, UdpSocket};
use std::time::Duration;

pub const FC_READ_COILS: u8 = 0x01;
pub const FC_READ_DISCRETE: u8 = 0x02;
pub const FC_READ_HOLDING: u8 = 0x03;
pub const FC_READ_INPUT: u8 = 0x04;
pub const FC_WRITE_COIL: u8 = 0x05;
pub const FC_WRITE_REG: u8 = 0x06;
pub const FC_DIAGNOSTICS: u8 = 0x08;
pub const FC_GET_COMM_EVENT_COUNTER: u8 = 0x0B;
pub const FC_WRITE_COILS: u8 = 0x0F;
pub const FC_WRITE_REGS: u8 = 0x10;
pub const FC_REPORT_SERVER_ID: u8 = 0x11;
pub const FC_MASK_WRITE_REG: u8 = 0x16;
pub const FC_READ_WRITE_REGS: u8 = 0x17;
pub const FC_READ_DEVICE_ID: u8 = 0x2B;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Tcp,
    Udp,
    Rtu,
    Ascii,
    RtuOverTcp,
    AsciiOverTcp,
}

impl Mode {
    pub fn is_serial(self) -> bool {
        matches!(self, Mode::Rtu | Mode::Ascii)
    }
    pub fn label(self) -> &'static str {
        match self {
            Mode::Tcp => "Modbus/TCP",
            Mode::Udp => "Modbus/UDP",
            Mode::Rtu => "Modbus RTU (serial)",
            Mode::Ascii => "Modbus ASCII (serial)",
            Mode::RtuOverTcp => "Modbus RTU over TCP",
            Mode::AsciiOverTcp => "Modbus ASCII over TCP",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnConfig {
    pub mode: Mode,
    pub host: String,
    pub port: u16,
    pub serial_port: String,
    pub baud: u32,
    pub data_bits: u8,
    pub parity: char, // 'N' | 'E' | 'O'
    pub stop_bits: u8,
    pub timeout_ms: u64,
    pub unit: u8,
}

impl Default for ConnConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Tcp,
            host: "127.0.0.1".into(),
            port: 502,
            serial_port: "/dev/ttyUSB0".into(),
            baud: 9600,
            data_bits: 8,
            parity: 'N',
            stop_bits: 1,
            timeout_ms: 1000,
            unit: 1,
        }
    }
}

/// CRC-16/MODBUS: poly 0xA001, init 0xFFFF, reflected; transmitted low byte first.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

/// LRC for Modbus ASCII: two's complement of the byte sum.
pub fn lrc(data: &[u8]) -> u8 {
    let sum: u8 = data.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    (!sum).wrapping_add(1)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02X}", b));
    }
    s
}

fn hex_decode(s: &str) -> Result<Vec<u8>> {
    let bytes = s.as_bytes();
    if bytes.len() % 2 != 0 {
        bail!("odd hex length");
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16).ok_or_else(|| anyhow::anyhow!("bad hex"))?;
        let lo = (bytes[i + 1] as char).to_digit(16).ok_or_else(|| anyhow::anyhow!("bad hex"))?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    Ok(out)
}

enum Kind {
    Tcp(TcpStream),
    Udp(UdpSocket),
    Serial(Box<dyn serialport::SerialPort>),
}

pub struct Transport {
    pub cfg: ConnConfig,
    kind: Kind,
    tid: u16,
}

impl Transport {
    pub fn connect(cfg: ConnConfig) -> Result<Self> {
        let to = Duration::from_millis(cfg.timeout_ms.max(1));
        let kind = match cfg.mode {
            Mode::Tcp | Mode::RtuOverTcp | Mode::AsciiOverTcp => {
                let s = TcpStream::connect((cfg.host.as_str(), cfg.port))?;
                s.set_read_timeout(Some(to))?;
                s.set_write_timeout(Some(to))?;
                s.set_nodelay(true).ok();
                Kind::Tcp(s)
            }
            Mode::Udp => {
                let s = UdpSocket::bind("0.0.0.0:0")?;
                s.connect((cfg.host.as_str(), cfg.port))?;
                s.set_read_timeout(Some(to))?;
                s.set_write_timeout(Some(to))?;
                Kind::Udp(s)
            }
            Mode::Rtu | Mode::Ascii => {
                use serialport::{DataBits, Parity, StopBits};
                let db = match cfg.data_bits {
                    5 => DataBits::Five,
                    6 => DataBits::Six,
                    7 => DataBits::Seven,
                    _ => DataBits::Eight,
                };
                let par = match cfg.parity.to_ascii_uppercase() {
                    'E' => Parity::Even,
                    'O' => Parity::Odd,
                    _ => Parity::None,
                };
                let sb = if cfg.stop_bits == 2 { StopBits::Two } else { StopBits::One };
                let p = serialport::new(cfg.serial_port.clone(), cfg.baud)
                    .data_bits(db)
                    .parity(par)
                    .stop_bits(sb)
                    .timeout(to)
                    .open()?;
                Kind::Serial(p)
            }
        };
        Ok(Self { cfg, kind, tid: 0 })
    }

    /// Build a wire frame for the given PDU.
    fn frame(&mut self, pdu: &[u8]) -> Vec<u8> {
        match self.cfg.mode {
            Mode::Tcp | Mode::Udp => {
                self.tid = self.tid.wrapping_add(1);
                let len = (pdu.len() + 1) as u16;
                let mut f = Vec::with_capacity(pdu.len() + 7);
                f.extend_from_slice(&self.tid.to_be_bytes());
                f.extend_from_slice(&0u16.to_be_bytes()); // protocol id
                f.extend_from_slice(&len.to_be_bytes());
                f.push(self.cfg.unit);
                f.extend_from_slice(pdu);
                f
            }
            Mode::Rtu | Mode::RtuOverTcp => {
                let mut f = Vec::with_capacity(pdu.len() + 3);
                f.push(self.cfg.unit);
                f.extend_from_slice(pdu);
                let c = crc16(&f);
                f.push((c & 0xFF) as u8);
                f.push((c >> 8) as u8);
                f
            }
            Mode::Ascii | Mode::AsciiOverTcp => {
                let mut body = Vec::with_capacity(pdu.len() + 1);
                body.push(self.cfg.unit);
                body.extend_from_slice(pdu);
                let l = lrc(&body);
                let mut s = String::from(":");
                s.push_str(&hex_encode(&body));
                s.push_str(&format!("{:02X}", l));
                s.push_str("\r\n");
                s.into_bytes()
            }
        }
    }

    fn raw_send(&mut self, data: &[u8]) -> Result<()> {
        match &mut self.kind {
            Kind::Tcp(s) => {
                s.write_all(data)?;
                s.flush().ok();
            }
            Kind::Udp(s) => {
                s.send(data)?;
            }
            Kind::Serial(p) => {
                p.write_all(data)?;
                p.flush()?;
            }
        }
        Ok(())
    }

    fn read_exact_n(&mut self, n: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; n];
        match &mut self.kind {
            Kind::Tcp(s) => s.read_exact(&mut buf)?,
            Kind::Udp(s) => {
                let got = s.recv(&mut buf)?;
                buf.truncate(got);
            }
            Kind::Serial(p) => p.read_exact(&mut buf)?,
        }
        Ok(buf)
    }

    fn drain(&mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut tmp = [0u8; 256];
        let old = match &mut self.kind {
            Kind::Serial(p) => {
                let o = p.timeout();
                p.set_timeout(Duration::from_millis(30)).ok();
                Some(o)
            }
            _ => None,
        };
        loop {
            let n = match &mut self.kind {
                Kind::Tcp(s) => s.read(&mut tmp).unwrap_or(0),
                Kind::Udp(s) => s.recv(&mut tmp).unwrap_or(0),
                Kind::Serial(p) => p.read(&mut tmp).unwrap_or(0),
            };
            if n == 0 {
                break;
            }
            out.extend_from_slice(&tmp[..n]);
            if out.len() > 512 {
                break;
            }
        }
        if let (Kind::Serial(p), Some(o)) = (&mut self.kind, old) {
            p.set_timeout(o).ok();
        }
        Ok(out)
    }

    fn recv_pdu(&mut self) -> Result<Vec<u8>> {
        match self.cfg.mode {
            Mode::Tcp | Mode::Udp => {
                let hdr = self.read_exact_n(7)?;
                let tid = u16::from_be_bytes([hdr[0], hdr[1]]);
                let proto = u16::from_be_bytes([hdr[2], hdr[3]]);
                let len = u16::from_be_bytes([hdr[4], hdr[5]]) as usize;
                if proto != 0 {
                    bail!("bad MBAP protocol id {proto}");
                }
                if tid != self.tid {
                    bail!("transaction id mismatch ({} != {})", tid, self.tid);
                }
                if len < 1 || len > 260 {
                    bail!("bad MBAP length {len}");
                }
                let pdu = self.read_exact_n(len - 1)?;
                Ok(pdu)
            }
            Mode::Rtu | Mode::RtuOverTcp => {
                let head = self.read_exact_n(2)?;
                let _unit = head[0];
                let fc = head[1];
                if fc & 0x80 != 0 {
                    let rest = self.read_exact_n(3)?; // code + crc
                    let body = [head[0], fc, rest[0], rest[1], rest[2]];
                    verify_crc(&body)?;
                    return Ok(vec![fc, rest[0]]);
                }
                match fc {
                    FC_READ_COILS | FC_READ_DISCRETE | FC_READ_HOLDING | FC_READ_INPUT => {
                        let bc = self.read_exact_n(1)?[0] as usize;
                        let data = self.read_exact_n(bc + 2)?; // data + crc
                        let mut body = vec![head[0], fc, bc as u8];
                        body.extend_from_slice(&data[..bc]);
                        body.extend_from_slice(&data[bc..bc + 2]);
                        verify_crc(&body)?;
                        let mut pdu = vec![fc, bc as u8];
                        pdu.extend_from_slice(&data[..bc]);
                        Ok(pdu)
                    }
                    FC_WRITE_COIL
                    | FC_WRITE_REG
                    | FC_WRITE_COILS
                    | FC_WRITE_REGS
                    | FC_MASK_WRITE_REG
                    | FC_DIAGNOSTICS
                    | FC_GET_COMM_EVENT_COUNTER => {
                        let data = self.read_exact_n(6)?; // 4 + crc
                        let mut body = vec![head[0], fc];
                        body.extend_from_slice(&data);
                        verify_crc(&body)?;
                        let mut pdu = vec![fc];
                        pdu.extend_from_slice(&data[..4]);
                        Ok(pdu)
                    }
                    _ => {
                        // variable length (Report Server ID, Read Device ID): CRC-drain.
                        let rest = self.drain()?;
                        let full = [&[head[0], fc][..], &rest].concat();
                        let full = full[..full.len().saturating_sub(2)].to_vec();
                        Ok(full[1..].to_vec())
                    }
                }
            }
            Mode::Ascii | Mode::AsciiOverTcp => {
                let mut line = Vec::new();
                let mut one = [0u8; 1];
                loop {
                    let n = match &mut self.kind {
                        Kind::Tcp(s) => s.read(&mut one).unwrap_or(0),
                        Kind::Udp(s) => s.recv(&mut one).unwrap_or(0),
                        Kind::Serial(p) => p.read(&mut one).unwrap_or(0),
                    };
                    if n == 0 {
                        break;
                    }
                    if one[0] == b'\n' {
                        break;
                    }
                    if one[0] != b'\r' {
                        line.push(one[0]);
                    }
                    if line.len() > 520 {
                        break;
                    }
                }
                if line.first() == Some(&b':') {
                    line.remove(0);
                }
                let raw = hex_decode(std::str::from_utf8(&line)?)?;
                if raw.len() < 2 {
                    bail!("short ASCII frame");
                }
                let (body, l) = raw.split_at(raw.len() - 1);
                if lrc(body) != l[0] {
                    bail!("ASCII LRC mismatch");
                }
                Ok(body[1..].to_vec())
            }
        }
    }

    pub fn transact(&mut self, pdu: &[u8]) -> Result<Vec<u8>> {
        let f = self.frame(pdu);
        self.raw_send(&f)?;
        let resp = self.recv_pdu()?;
        check_exception(&resp)?;
        Ok(resp)
    }

    // ---- high level helpers -------------------------------------------------

    pub fn read_bits(&mut self, fc: u8, addr: u16, qty: u16) -> Result<Vec<bool>> {
        let resp = self.transact(&[fc, hi(addr), lo(addr), hi(qty), lo(qty)])?;
        let bc = resp[1] as usize;
        let data = &resp[2..2 + bc];
        Ok(unpack_bits(data, qty as usize))
    }

    pub fn read_regs(&mut self, fc: u8, addr: u16, qty: u16) -> Result<Vec<u16>> {
        let resp = self.transact(&[fc, hi(addr), lo(addr), hi(qty), lo(qty)])?;
        let bc = resp[1] as usize;
        let data = &resp[2..2 + bc];
        Ok(data
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect())
    }

    pub fn write_single_coil(&mut self, addr: u16, on: bool) -> Result<()> {
        let v: u16 = if on { 0xFF00 } else { 0x0000 };
        self.transact(&[FC_WRITE_COIL, hi(addr), lo(addr), hi(v), lo(v)])?;
        Ok(())
    }

    pub fn write_single_reg(&mut self, addr: u16, val: u16) -> Result<()> {
        self.transact(&[FC_WRITE_REG, hi(addr), lo(addr), hi(val), lo(val)])?;
        Ok(())
    }

    pub fn write_regs(&mut self, addr: u16, vals: &[u16]) -> Result<()> {
        let qty = vals.len() as u16;
        let mut pdu = vec![FC_WRITE_REGS, hi(addr), lo(addr), hi(qty), lo(qty), (vals.len() * 2) as u8];
        for v in vals {
            pdu.push(hi(*v));
            pdu.push(lo(*v));
        }
        self.transact(&pdu)?;
        Ok(())
    }

    pub fn write_coils(&mut self, addr: u16, vals: &[bool]) -> Result<()> {
        let qty = vals.len() as u16;
        let packed = pack_bits(vals);
        let mut pdu = vec![FC_WRITE_COILS, hi(addr), lo(addr), hi(qty), lo(qty), packed.len() as u8];
        pdu.extend_from_slice(&packed);
        self.transact(&pdu)?;
        Ok(())
    }

    pub fn mask_write_reg(&mut self, addr: u16, and_mask: u16, or_mask: u16) -> Result<()> {
        self.transact(&[
            FC_MASK_WRITE_REG,
            hi(addr),
            lo(addr),
            hi(and_mask),
            lo(and_mask),
            hi(or_mask),
            lo(or_mask),
        ])?;
        Ok(())
    }

    pub fn read_write_regs(
        &mut self,
        read_addr: u16,
        read_qty: u16,
        write_addr: u16,
        write_vals: &[u16],
    ) -> Result<Vec<u16>> {
        let wq = write_vals.len() as u16;
        let mut pdu = vec![
            FC_READ_WRITE_REGS,
            hi(read_addr),
            lo(read_addr),
            hi(read_qty),
            lo(read_qty),
            hi(write_addr),
            lo(write_addr),
            hi(wq),
            lo(wq),
            (write_vals.len() * 2) as u8,
        ];
        for v in write_vals {
            pdu.push(hi(*v));
            pdu.push(lo(*v));
        }
        let resp = self.transact(&pdu)?;
        let bc = resp[1] as usize;
        let data = &resp[2..2 + bc];
        Ok(data
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect())
    }

    pub fn report_server_id(&mut self) -> Result<Vec<u8>> {
        let resp = self.transact(&[FC_REPORT_SERVER_ID])?;
        Ok(resp[1..].to_vec())
    }

    pub fn read_device_identification(&mut self, category: u8) -> Result<Vec<u8>> {
        let resp = self.transact(&[FC_READ_DEVICE_ID, 0x0E, category, 0x00])?;
        Ok(resp)
    }
}

fn verify_crc(frame: &[u8]) -> Result<()> {
    if frame.len() < 3 {
        bail!("short RTU frame");
    }
    let n = frame.len();
    let got = u16::from_le_bytes([frame[n - 2], frame[n - 1]]);
    let want = crc16(&frame[..n - 2]);
    if got != want {
        bail!("RTU CRC mismatch: {:04X} != {:04X}", got, want);
    }
    Ok(())
}

fn hi(v: u16) -> u8 {
    (v >> 8) as u8
}
fn lo(v: u16) -> u8 {
    (v & 0xFF) as u8
}

pub fn unpack_bits(data: &[u8], qty: usize) -> Vec<bool> {
    let mut out = Vec::with_capacity(qty);
    for i in 0..qty {
        let byte = data[i / 8];
        out.push((byte >> (i % 8)) & 1 == 1);
    }
    out
}

pub fn pack_bits(bits: &[bool]) -> Vec<u8> {
    let n = bits.len().div_ceil(8);
    let mut out = vec![0u8; n];
    for (i, b) in bits.iter().enumerate() {
        if *b {
            out[i / 8] |= 1 << (i % 8);
        }
    }
    out
}

pub fn check_exception(pdu: &[u8]) -> Result<()> {
    if pdu.is_empty() {
        bail!("empty PDU");
    }
    if pdu[0] & 0x80 != 0 {
        let code = pdu.get(1).copied().unwrap_or(0);
        bail!("Modbus exception {} ({})", code, exception_name(code));
    }
    Ok(())
}

pub fn exception_name(code: u8) -> &'static str {
    match code {
        0x01 => "Illegal function",
        0x02 => "Illegal data address",
        0x03 => "Illegal data value",
        0x04 => "Server device failure",
        0x05 => "Acknowledge",
        0x06 => "Server device busy",
        0x07 => "Negative acknowledge",
        0x08 => "Memory parity error",
        0x0A => "Gateway path unavailable",
        0x0B => "Gateway target device failed to respond",
        _ => "Unknown exception",
    }
}

pub fn fc_label(fc: u8) -> &'static str {
    match fc {
        FC_READ_COILS => "01 Read Coils",
        FC_READ_DISCRETE => "02 Read Discrete Inputs",
        FC_READ_HOLDING => "03 Read Holding Registers",
        FC_READ_INPUT => "04 Read Input Registers",
        FC_WRITE_COIL => "05 Write Single Coil",
        FC_WRITE_REG => "06 Write Single Register",
        FC_DIAGNOSTICS => "08 Diagnostics",
        FC_GET_COMM_EVENT_COUNTER => "11 Get Comm Event Counter",
        FC_WRITE_COILS => "15 Write Multiple Coils",
        FC_WRITE_REGS => "16 Write Multiple Registers",
        FC_REPORT_SERVER_ID => "17 Report Server ID",
        FC_MASK_WRITE_REG => "22 Mask Write Register",
        FC_READ_WRITE_REGS => "23 Read/Write Multiple Registers",
        FC_READ_DEVICE_ID => "43/14 Read Device Identification",
        _ => "Unknown function",
    }
}

pub fn is_bit_fc(fc: u8) -> bool {
    matches!(fc, FC_READ_COILS | FC_READ_DISCRETE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_vector() {
        // "01 03 00 00 00 0A" -> CRC 0xCDC5 -> on the wire C5 CD
        let c = crc16(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x0A]);
        assert_eq!(c, 0xCDC5);
    }

    #[test]
    fn lrc_roundtrip() {
        let body = [0x01u8, 0x03, 0x00, 0x00, 0x00, 0x0A];
        let l = lrc(&body);
        let mut b = body.to_vec();
        b.push(l);
        assert_eq!(lrc(&b), 0);
    }

    #[test]
    fn bits_roundtrip() {
        let v = vec![true, false, true, true, false, false, false, true, true];
        let p = pack_bits(&v);
        assert_eq!(unpack_bits(&p, v.len()), v);
    }
}
