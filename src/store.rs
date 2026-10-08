//! Runtime store + polling worker.
//!
//! One worker thread owns the transport and polls all enabled groups on their
//! own scan rates. UI reads the shared snapshot; commands (writes, add/remove
//! groups) go to the worker over an mpsc channel.

use crate::colors::ColorMap;
use crate::formats::ValueFormat;
use crate::modbus::{
    is_bit_fc, ConnConfig, Mode, Transport, FC_READ_COILS, FC_READ_DISCRETE, FC_READ_HOLDING,
    FC_READ_INPUT, FC_WRITE_COIL, FC_WRITE_REG, FC_WRITE_REGS,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct CellKey {
    pub unit: u8,
    pub fc: u8,
    pub addr: u16,
}

impl CellKey {
    pub fn new(unit: u8, fc: u8, addr: u16) -> Self {
        Self { unit, fc, addr }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Scale {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub decimals: u8,
    pub enabled: bool,
}

impl Default for Scale {
    fn default() -> Self {
        Self { x1: 0.0, y1: 0.0, x2: 100.0, y2: 100.0, decimals: 2, enabled: false }
    }
}

impl Scale {
    pub fn apply(&self, x: f64) -> f64 {
        if !self.enabled || (self.x2 - self.x1).abs() < f64::EPSILON {
            x
        } else {
            (self.y2 - self.y1) / (self.x2 - self.x1) * (x - self.x1) + self.y1
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PollGroup {
    pub id: u32,
    pub unit: u8,
    pub fc: u8,
    pub start: u16,
    pub count: u16,
    pub scan_ms: u64,
    pub enabled: bool,
    pub format: ValueFormat,
    pub color: ColorMap,
    pub scale: Scale,
    pub name: String,
    pub chart: bool,
}

impl PollGroup {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            unit: 1,
            fc: FC_READ_HOLDING,
            start: 0,
            count: 10,
            scan_ms: 1000,
            enabled: true,
            format: ValueFormat::U16,
            color: ColorMap::default(),
            scale: Scale::default(),
            name: format!("Group {id}"),
            chart: true,
        }
    }
    pub fn is_bit(&self) -> bool {
        is_bit_fc(self.fc)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Stats {
    pub ok: u64,
    pub err: u64,
    pub timeouts: u64,
    pub last_ms: f64,
    pub last_error: String,
    pub last_exception: String,
    pub polls: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrafficEntry {
    pub t: String,
    pub dir: char, // '>' tx, '<' rx, '!' error
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScanState {
    pub running: bool,
    pub kind: u8, // 0 = address scan, 1 = slave scan
    pub cur: u16,
    pub end: u16,
    pub found: Vec<(u16, String)>,
    pub errors: u32,
}

pub struct Shared {
    pub words: HashMap<CellKey, u16>,
    pub bits: HashMap<CellKey, bool>,
    pub history: HashMap<CellKey, VecDeque<f64>>,
    pub stats: Stats,
    pub traffic: VecDeque<TrafficEntry>,
    pub conn: ConnState,
    pub scan: ScanState,
    pub last_response: String,
    pub comm_event: Option<(u16, u16)>,
    pub device_id: Vec<(u8, String)>,
    pub server_id: Vec<u8>,
    pub diag: Option<(u16, u16)>,
    pub scan_cancel: Arc<AtomicBool>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            words: HashMap::new(),
            bits: HashMap::new(),
            history: HashMap::new(),
            stats: Stats::default(),
            traffic: VecDeque::with_capacity(512),
            conn: ConnState::Disconnected,
            scan: ScanState::default(),
            last_response: String::new(),
            comm_event: None,
            device_id: Vec::new(),
            server_id: Vec::new(),
            diag: None,
            scan_cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

pub type SharedHandle = Arc<Mutex<Shared>>;

impl Shared {
    pub fn word(&self, k: CellKey) -> Option<u16> {
        self.words.get(&k).copied()
    }
    pub fn bit(&self, k: CellKey) -> Option<bool> {
        self.bits.get(&k).copied()
    }
}

#[derive(Debug)]
pub enum Cmd {
    AddGroup(PollGroup),
    UpdateGroup(PollGroup),
    RemoveGroup(u32),
    ClearGroups,
    WriteReg { unit: u8, addr: u16, value: u16 },
    WriteRegs { unit: u8, addr: u16, values: Vec<u16> },
    WriteCoil { unit: u8, addr: u16, on: bool },
    WriteCoils { unit: u8, addr: u16, values: Vec<bool> },
    MaskWrite { unit: u8, addr: u16, and_mask: u16, or_mask: u16 },
    ReadWriteMulti { unit: u8, read_addr: u16, read_qty: u16, write_addr: u16, values: Vec<u16> },
    Raw { unit: u8, pdu: Vec<u8> },
    Diagnostics { unit: u8, sub: u16, data: u16 },
    CommEvent { unit: u8 },
    ReportId { unit: u8 },
    DeviceId { unit: u8, code: u8, obj: u8 },
    ScanAddress { unit: u8, fc: u8, start: u16, end: u16 },
    ScanSlave { fc: u8, addr: u16, start: u8, end: u8 },
    CancelScan,
    SetUnit(u8),
    Stop,
}

fn ts() -> String {
    let now = std::time::SystemTime::now();
    let d = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() % 86400;
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

fn hexs(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

fn push_traffic(sh: &SharedHandle, dir: char, text: String) {
    if let Ok(mut s) = sh.lock() {
        if s.traffic.len() >= 500 {
            s.traffic.pop_front();
        }
        s.traffic.push_back(TrafficEntry { t: ts(), dir, text });
    }
}

/// Spawn the polling worker. Returns the command sender and the join handle.
pub fn spawn(cfg: ConnConfig, shared: SharedHandle) -> (Sender<Cmd>, JoinHandle<()>) {
    let (tx, rx) = channel::<Cmd>();
    let handle = std::thread::Builder::new()
        .name("mbrs-poll".into())
        .spawn(move || worker(cfg, shared, rx))
        .expect("spawn poll worker");
    (tx, handle)
}

fn worker(cfg: ConnConfig, shared: SharedHandle, rx: Receiver<Cmd>) {
    let mut groups: Vec<PollGroup> = Vec::new();
    let mut next_id: u32 = 1;
    let mut transport: Option<Transport> = None;
    let mut next_due: HashMap<u32, Instant> = HashMap::new();
    let mut last_reconnect = Instant::now() - Duration::from_secs(10);
    let mut cfg = cfg;

    loop {
        // Drain commands.
        loop {
            match rx.recv_timeout(Duration::from_millis(2)) {
                Ok(Cmd::Stop) => return,
                Ok(Cmd::SetUnit(u)) => {
                    cfg.unit = u;
                    transport = None;
                }
                Ok(Cmd::AddGroup(mut g)) => {
                    if g.id == 0 || groups.iter().any(|x| x.id == g.id) {
                        g.id = next_id;
                    }
                    next_id = next_id.max(g.id + 1);
                    next_due.insert(g.id, Instant::now());
                    groups.push(g);
                }
                Ok(Cmd::UpdateGroup(g)) => {
                    if let Some(slot) = groups.iter_mut().find(|x| x.id == g.id) {
                        *slot = g;
                    }
                }
                Ok(Cmd::RemoveGroup(id)) => {
                    groups.retain(|g| g.id != id);
                    next_due.remove(&id);
                }
                Ok(Cmd::ClearGroups) => {
                    groups.clear();
                    next_due.clear();
                }
                Ok(Cmd::WriteReg { unit, addr, value }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    if let Some(t) = transport.as_mut() {
                        t.cfg.unit = unit;
                        match t.write_single_reg(addr, value) {
                            Ok(()) => push_traffic(&shared, '>', format!("WRITE reg {addr} = {value}")),
                            Err(e) => push_traffic(&shared, '!', format!("WRITE reg {addr} failed: {e}")),
                        }
                    }
                }
                Ok(Cmd::WriteRegs { unit, addr, values }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    if let Some(t) = transport.as_mut() {
                        t.cfg.unit = unit;
                        match t.write_regs(addr, &values) {
                            Ok(()) => push_traffic(&shared, '>', format!("WRITE {} regs @ {addr}", values.len())),
                            Err(e) => push_traffic(&shared, '!', format!("WRITE regs @ {addr} failed: {e}")),
                        }
                    }
                }
                Ok(Cmd::WriteCoil { unit, addr, on }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    if let Some(t) = transport.as_mut() {
                        t.cfg.unit = unit;
                        match t.write_single_coil(addr, on) {
                            Ok(()) => push_traffic(&shared, '>', format!("WRITE coil {addr} = {on}")),
                            Err(e) => push_traffic(&shared, '!', format!("WRITE coil {addr} failed: {e}")),
                        }
                    }
                }
                Ok(Cmd::WriteCoils { unit, addr, values }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    if let Some(t) = transport.as_mut() {
                        t.cfg.unit = unit;
                        match t.write_coils(addr, &values) {
                            Ok(()) => push_traffic(&shared, '>', format!("WRITE {} coils @ {addr}", values.len())),
                            Err(e) => push_traffic(&shared, '!', format!("WRITE coils @ {addr} failed: {e}")),
                        }
                    }
                }
                Ok(Cmd::MaskWrite { unit, addr, and_mask, or_mask }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    if let Some(t) = transport.as_mut() {
                        t.cfg.unit = unit;
                        match t.mask_write_reg(addr, and_mask, or_mask) {
                            Ok(()) => push_traffic(&shared, '>', format!("MASK WRITE {addr} and={and_mask:04X} or={or_mask:04X}")),
                            Err(e) => push_traffic(&shared, '!', format!("mask write failed: {e}")),
                        }
                    }
                }
                Ok(Cmd::ReadWriteMulti { unit, read_addr, read_qty, write_addr, values }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    if let Some(t) = transport.as_mut() {
                        t.cfg.unit = unit;
                        match t.read_write_regs(read_addr, read_qty, write_addr, &values) {
                            Ok(v) => {
                                for (i, val) in v.iter().enumerate() {
                                    if let Ok(mut s) = shared.lock() {
                                        s.words.insert(CellKey::new(unit, FC_READ_HOLDING, read_addr.wrapping_add(i as u16)), *val);
                                    }
                                }
                                push_traffic(&shared, '<', format!("RW read {} regs", v.len()));
                            }
                            Err(e) => push_traffic(&shared, '!', format!("read/write failed: {e}")),
                        }
                    }
                }
                Ok(Cmd::Raw { unit, pdu }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    if let Some(t) = transport.as_mut() {
                        t.cfg.unit = unit;
                        push_traffic(&shared, '>', format!("TX {}", hexs(&pdu)));
                        match t.raw(&pdu) {
                            Ok(r) => {
                                let h = hexs(&r);
                                push_traffic(&shared, '<', format!("RX {h}"));
                                if let Ok(mut s) = shared.lock() {
                                    s.last_response = h;
                                }
                            }
                            Err(e) => {
                                push_traffic(&shared, '!', format!("raw failed: {e}"));
                                if let Ok(mut s) = shared.lock() {
                                    s.last_response = format!("error: {e}");
                                }
                            }
                        }
                    }
                }
                Ok(Cmd::Diagnostics { unit, sub, data }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    if let Some(t) = transport.as_mut() {
                        t.cfg.unit = unit;
                        match t.diagnostics(sub, data) {
                            Ok(v) => {
                                push_traffic(&shared, '<', format!("DIAG {sub:#06X} = {v}"));
                                if let Ok(mut s) = shared.lock() { s.diag = Some((sub, v)); }
                            }
                            Err(e) => push_traffic(&shared, '!', format!("diag failed: {e}")),
                        }
                    }
                }
                Ok(Cmd::CommEvent { unit }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    if let Some(t) = transport.as_mut() {
                        t.cfg.unit = unit;
                        match t.get_comm_event_counter() {
                            Ok(v) => { if let Ok(mut s) = shared.lock() { s.comm_event = Some(v); } }
                            Err(e) => push_traffic(&shared, '!', format!("0B failed: {e}")),
                        }
                    }
                }
                Ok(Cmd::ReportId { unit }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    if let Some(t) = transport.as_mut() {
                        t.cfg.unit = unit;
                        match t.report_server_id() {
                            Ok(v) => { if let Ok(mut s) = shared.lock() { s.server_id = v; } }
                            Err(e) => push_traffic(&shared, '!', format!("11 failed: {e}")),
                        }
                    }
                }
                Ok(Cmd::DeviceId { unit, code, obj }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    if let Some(t) = transport.as_mut() {
                        t.cfg.unit = unit;
                        match t.device_identification(code, obj) {
                            Ok((objs, _more)) => { if let Ok(mut s) = shared.lock() { s.device_id = objs; } }
                            Err(e) => push_traffic(&shared, '!', format!("2B/0E failed: {e}")),
                        }
                    }
                }
                Ok(Cmd::ScanAddress { unit, fc, start, end }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    let cancel = {
                        let mut s = shared.lock().unwrap();
                        s.scan = ScanState { running: true, kind: 0, cur: start, end, found: Vec::new(), errors: 0 };
                        s.scan_cancel.store(false, Ordering::SeqCst);
                        s.scan_cancel.clone()
                    };
                    if let Some(t) = transport.as_mut() {
                        let old = t.cfg.timeout_ms;
                        t.cfg.timeout_ms = old.min(250);
                        do_scan_address(t, &shared, &cancel, unit, fc, start, end);
                        t.cfg.timeout_ms = old;
                    }
                    if let Ok(mut s) = shared.lock() { s.scan.running = false; }
                }
                Ok(Cmd::ScanSlave { fc, addr, start, end }) => {
                    ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
                    let cancel = {
                        let mut s = shared.lock().unwrap();
                        s.scan = ScanState { running: true, kind: 1, cur: start as u16, end: end as u16, found: Vec::new(), errors: 0 };
                        s.scan_cancel.store(false, Ordering::SeqCst);
                        s.scan_cancel.clone()
                    };
                    if let Some(t) = transport.as_mut() {
                        let old = t.cfg.timeout_ms;
                        t.cfg.timeout_ms = old.min(250);
                        do_scan_slave(t, &shared, &cancel, fc, addr, start, end);
                        t.cfg.timeout_ms = old;
                    }
                    if let Ok(mut s) = shared.lock() { s.scan.running = false; }
                }
                Ok(Cmd::CancelScan) => {
                    if let Ok(s) = shared.lock() { s.scan_cancel.store(true, Ordering::SeqCst); }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }

        ensure_conn(&mut transport, &cfg, &shared, &mut last_reconnect);
        let Some(t) = transport.as_mut() else {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        };

        let now = Instant::now();
        let due: Vec<PollGroup> = groups
            .iter()
            .filter(|g| g.enabled && next_due.get(&g.id).map(|d| *d <= now).unwrap_or(true))
            .cloned()
            .collect();

        if due.is_empty() {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }

        for g in due {
            let t0 = Instant::now();
            let res = poll_group(t, &g, &shared);
            let dt = t0.elapsed().as_secs_f64() * 1000.0;
            if let Ok(mut s) = shared.lock() {
                match res {
                    Ok(()) => {
                        s.stats.ok += 1;
                        s.stats.polls += 1;
                        s.stats.last_ms = dt;
                    }
                    Err(e) => {
                        s.stats.err += 1;
                        s.stats.last_error = e.to_string();
                        if e.to_string().contains("timed out") || e.to_string().contains("timeout") {
                            s.stats.timeouts += 1;
                        }
                    }
                }
            }
            let due_at = Instant::now() + Duration::from_millis(g.scan_ms.max(1));
            next_due.insert(g.id, due_at);
        }
    }
}

fn ensure_conn(
    transport: &mut Option<Transport>,
    cfg: &ConnConfig,
    shared: &SharedHandle,
    last_reconnect: &mut Instant,
) {
    if transport.is_some() {
        return;
    }
    if last_reconnect.elapsed() < Duration::from_millis(1500) {
        return;
    }
    *last_reconnect = Instant::now();
    if let Ok(mut s) = shared.lock() {
        s.conn = ConnState::Connecting;
    }
    match Transport::connect(cfg.clone()) {
        Ok(t) => {
            if let Ok(mut s) = shared.lock() {
                s.conn = ConnState::Connected;
            }
            push_traffic(shared, '>', format!("connected: {}", cfg.mode.label()));
            *transport = Some(t);
        }
        Err(e) => {
            if let Ok(mut s) = shared.lock() {
                s.conn = ConnState::Error(e.to_string());
            }
        }
    }
}

fn poll_group(t: &mut Transport, g: &PollGroup, shared: &SharedHandle) -> Result<()> {
    t.cfg.unit = g.unit;
    if g.is_bit() {
        let fc = if g.fc == FC_READ_COILS { FC_READ_COILS } else { FC_READ_DISCRETE };
        let bits = t.read_bits(fc, g.start, g.count)?;
        if let Ok(mut s) = shared.lock() {
            for (i, b) in bits.iter().enumerate() {
                s.bits.insert(CellKey::new(g.unit, g.fc, g.start + i as u16), *b);
            }
        }
    } else {
        let fc = if g.fc == FC_READ_INPUT { FC_READ_INPUT } else { FC_READ_HOLDING };
        let regs = t.read_regs(fc, g.start, g.count)?;
        if let Ok(mut s) = shared.lock() {
            for (i, v) in regs.iter().enumerate() {
                s.words.insert(CellKey::new(g.unit, g.fc, g.start + i as u16), *v);
            }
            if g.chart {
                let key = CellKey::new(g.unit, g.fc, g.start);
                let numeric = g.format.numeric(&regs);
                let val = g.scale.apply(numeric);
                let q = s.history.entry(key).or_insert_with(VecDeque::new);
                q.push_back(val);
                while q.len() > 2000 {
                    q.pop_front();
                }
            }
        }
    }
    Ok(())
}

pub fn label_for_fc(fc: u8) -> &'static str {
    crate::modbus::fc_label(fc)
}

/// Convert a register-or-coil read into an "f64 view" for colouring.
pub fn value_of(shared: &Shared, k: CellKey, fmt: ValueFormat) -> f64 {
    if is_bit_fc(k.fc) {
        shared.bits.get(&k).map(|b| if *b { 1.0 } else { 0.0 }).unwrap_or(f64::NAN)
    } else {
        let regs: Vec<u16> = (0..fmt.regs_needed() as u16)
            .filter_map(|o| shared.words.get(&CellKey::new(k.unit, k.fc, k.addr.wrapping_add(o))).copied())
            .collect();
        if regs.is_empty() {
            f64::NAN
        } else {
            fmt.numeric(&regs)
        }
    }
}

pub fn regs_of(shared: &Shared, k: CellKey, fmt: ValueFormat) -> Vec<u16> {
    (0..fmt.regs_needed() as u16)
        .filter_map(|o| shared.words.get(&CellKey::new(k.unit, k.fc, k.addr.wrapping_add(o))).copied())
        .collect()
}

pub const WRITE_FCS: &[u8] = &[FC_WRITE_COIL, FC_WRITE_REG, FC_WRITE_REGS];

pub fn mode_default_unit(_m: Mode) -> u8 {
    1
}

/// True when the cell's function code addresses bits (coils / discrete inputs).
pub fn is_bit_fc_key(k: CellKey) -> bool {
    is_bit_fc(k.fc)
}

fn do_scan_address(
    t: &mut Transport,
    shared: &SharedHandle,
    cancel: &Arc<AtomicBool>,
    unit: u8,
    fc: u8,
    start: u16,
    end: u16,
) {
    t.cfg.unit = unit;
    let mut a = start;
    loop {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        if let Ok(mut s) = shared.lock() {
            s.scan.cur = a;
        }
        let res = if is_bit_fc(fc) {
            t.read_bits(fc, a, 1).map(|b| if b[0] { "ON".to_string() } else { "off".to_string() })
        } else {
            t.read_regs(fc, a, 1).map(|v| v[0].to_string())
        };
        if let Ok(mut s) = shared.lock() {
            match res {
                Ok(v) => s.scan.found.push((a, v)),
                Err(_) => s.scan.errors += 1,
            }
        }
        if a == end || a == u16::MAX {
            break;
        }
        a = a.wrapping_add(1);
    }
}

fn do_scan_slave(
    t: &mut Transport,
    shared: &SharedHandle,
    cancel: &Arc<AtomicBool>,
    fc: u8,
    addr: u16,
    start: u8,
    end: u8,
) {
    for u in start..=end {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        t.cfg.unit = u;
        if let Ok(mut s) = shared.lock() {
            s.scan.cur = u as u16;
        }
        let ok = if is_bit_fc(fc) {
            t.read_bits(fc, addr, 1).is_ok()
        } else {
            t.read_regs(fc, addr, 1).is_ok()
        };
        if let Ok(mut s) = shared.lock() {
            if ok {
                s.scan.found.push((u as u16, format!("slave {u}")));
            } else {
                s.scan.errors += 1;
            }
        }
    }
}
