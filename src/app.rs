//! egui front-end: register grid, conditional colours, SCADA canvas, strip
//! chart, communication traffic, address/slave scan, test center, logging and
//! the full Modbus Poll dialog set, with matching menu bar and shortcuts.

use crate::colors::{contrast, ColorMode, ColorOp, ColorRule, Palette, PALETTE32};
use crate::formats::{ValueFormat, WordOrder};
use crate::logging::{LogConfig, LogFormat, LogPolicy, LogWriter};
use crate::modbus::{
    diag_sub_name, fc_label, is_bit_fc, ConnConfig, Mode, FC_GET_COMM_EVENT_COUNTER, FC_READ_COILS,
    FC_READ_DEVICE_ID, FC_READ_DISCRETE, FC_READ_HOLDING, FC_READ_INPUT, FC_REPORT_SERVER_ID,
};
use crate::names::Names;
use crate::scada::{autolayout, Tile, TileKind};
use crate::store::{is_bit_fc_key, CellKey, Cmd, ConnState, PollGroup, Shared, SharedHandle};
use crate::workspace::Project;
use eframe::egui;
use egui::{Align2, Color32, FontId, Key, Modifiers, Stroke};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Grid,
    Scada,
    Chart,
    Traffic,
    Scan,
    Test,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Modal {
    None,
    Connection,
    GroupDef,
    SetSlaveAll,
    SetScanRateAll,
    Colors,
    Scaling,
    Font,
    Names,
    BinaryNames,
    Log,
    ExcelLog,
    Series,
    ErrorCounters,
    Advanced,
    Security,
    About,
    WriteCoil,
    WriteReg,
    WriteRegBinary,
    WriteInteger,
    WriteFloat,
    WriteMultiCoils,
    WriteMultiRegs,
    MaskWrite,
    ReadWriteMulti,
    Diagnostics,
    CommEvent,
    ReportId,
    DeviceId,
    AddressScan,
    SlaveScan,
    TestCenter,
    Strings,
}

struct Snap {
    words: HashMap<CellKey, u16>,
    bits: HashMap<CellKey, bool>,
    history: HashMap<CellKey, VecDeque<f64>>,
    stats: crate::store::Stats,
    conn: ConnState,
    scan: crate::store::ScanState,
    last_response: String,
    comm_event: Option<(u16, u16)>,
    device_id: Vec<(u8, String)>,
    server_id: Vec<u8>,
    diag: Option<(u16, u16)>,
}

pub struct MbApp {
    project: Project,
    shared: SharedHandle,
    cmd: Option<Sender<Cmd>>,
    worker: Option<std::thread::JoinHandle<()>>,

    view: View,
    modal: Modal,
    selected: Option<CellKey>,
    write_buf: String,
    path_buf: String,
    status: String,
    active_group: usize,
    sel_tile: Option<usize>,

    // dialog buffers
    g_edit: PollGroup,
    w_unit: u8,
    w_addr: u16,
    w_qty: u16,
    w_val: String,
    w_and: u16,
    w_or: u16,
    diag_sub: u16,
    diag_data: u16,
    devid_code: u8,
    devid_obj: u8,
    scan_fc: u8,
    scan_unit: u8,
    scan_start: u16,
    scan_end: u16,
    slave_start: u8,
    slave_end: u8,
    slave_addr: u16,
    test_unit: u8,
    test_pdu: String,
    test_ascii: String,
    test_comment: String,
    all_unit: u8,
    all_scan: u64,

    // names / logging / series / display
    names: Names,
    new_name_key: u16,
    new_name_val: String,
    names_path: String,
    log: LogConfig,
    logw: LogWriter,
    last_log: Instant,
    series_points: usize,
    series_auto: bool,
    series_ymin: f64,
    series_ymax: f64,
    plc_addr: bool,
    show_bits: bool,
}

impl MbApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        let shared: SharedHandle = Arc::new(Mutex::new(Shared::default()));
        let project = Project::default();
        let (cmd, worker) = crate::store::spawn(project.conn.clone(), shared.clone());
        for g in &project.groups {
            let _ = cmd.send(Cmd::AddGroup(g.clone()));
        }
        Self {
            project,
            shared,
            cmd: Some(cmd),
            worker: Some(worker),
            view: View::Grid,
            modal: Modal::None,
            selected: None,
            write_buf: String::new(),
            path_buf: "/home/lain/mbrs-workspace.mbw".into(),
            status: "ready".into(),
            active_group: 0,
            sel_tile: None,
            g_edit: PollGroup::new(1),
            w_unit: 1,
            w_addr: 0,
            w_qty: 1,
            w_val: String::new(),
            w_and: 0xFFFF,
            w_or: 0,
            diag_sub: 0,
            diag_data: 0,
            devid_code: 1,
            devid_obj: 0,
            scan_fc: FC_READ_HOLDING,
            scan_unit: 1,
            scan_start: 0,
            scan_end: 19,
            slave_start: 1,
            slave_end: 32,
            slave_addr: 0,
            test_unit: 1,
            test_pdu: "03 00 00 00 02".into(),
            test_ascii: String::new(),
            test_comment: String::new(),
            all_unit: 1,
            all_scan: 1000,
            names: Names::default(),
            new_name_key: 0,
            new_name_val: String::new(),
            names_path: "/home/lain/mbrs-names.txt".into(),
            log: LogConfig::default(),
            logw: LogWriter::new(),
            last_log: Instant::now(),
            series_points: 500,
            series_auto: true,
            series_ymin: 0.0,
            series_ymax: 100.0,
            plc_addr: false,
            show_bits: false,
        }
    }

    pub fn load_workspace_file(&mut self, path: &str) {
        self.path_buf = path.to_string();
        self.project_open();
    }

    fn project_save(&mut self) {
        self.project.names = self.names.clone();
        self.project.log = self.log.clone();
        match self.project.save(&self.path_buf) {
            Ok(_) => self.status = format!("saved {}", self.path_buf),
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }

    fn project_open(&mut self) {
        match Project::load(&self.path_buf) {
            Ok(p) => {
                self.project = p;
                self.names = self.project.names.clone();
                self.log = self.project.log.clone();
                self.active_group = 0;
                self.restart_worker();
                self.status = format!("loaded {}", self.path_buf);
            }
            Err(e) => self.status = format!("load failed: {e}"),
        }
    }

    fn send(&self, c: Cmd) {
        if let Some(tx) = &self.cmd {
            let _ = tx.send(c);
        }
    }

    fn restart_worker(&mut self) {
        if let Some(tx) = self.cmd.take() {
            let _ = tx.send(Cmd::Stop);
        }
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
        let (cmd, worker) = crate::store::spawn(self.project.conn.clone(), self.shared.clone());
        for g in &self.project.groups {
            let _ = cmd.send(Cmd::AddGroup(g.clone()));
        }
        self.cmd = Some(cmd);
        self.worker = Some(worker);
    }

    fn snapshot(&self, with_history: bool) -> Snap {
        let s = self.shared.lock().unwrap();
        Snap {
            words: s.words.clone(),
            bits: s.bits.clone(),
            history: if with_history { s.history.clone() } else { HashMap::new() },
            stats: s.stats.clone(),
            conn: s.conn.clone(),
            scan: s.scan.clone(),
            last_response: s.last_response.clone(),
            comm_event: s.comm_event,
            device_id: s.device_id.clone(),
            server_id: s.server_id.clone(),
            diag: s.diag,
        }
    }

    fn active(&self) -> Option<&PollGroup> {
        self.project.groups.get(self.active_group)
    }

    fn set_format_all_selected(&mut self, f: ValueFormat, pending: &mut Vec<Cmd>) {
        if let Some(g) = self.project.groups.get_mut(self.active_group) {
            g.format = f;
            pending.push(Cmd::UpdateGroup(g.clone()));
        }
    }

    fn toggle_log(&mut self, on: bool) {
        self.log.enabled = on;
        if !on {
            self.logw.close();
        }
        self.status = if on { format!("logging -> {}", self.log.path) } else { "logging off".into() };
    }

    fn do_log_tick(&mut self) {
        if !self.log.enabled {
            return;
        }
        let now = Instant::now();
        if now.duration_since(self.last_log).as_millis() < self.log.interval_ms as u128 {
            return;
        }
        self.last_log = now;
        let (g, snap) = match self.active() {
            Some(g) => (g.clone(), self.shared.lock().unwrap().words.clone()),
            None => return,
        };
        let n = g.format.regs_needed() as u16;
        let mut header = vec!["address".to_string()];
        let mut row = vec![format!("{}", g.start)];
        for i in 0..g.count {
            let addr = g.start.wrapping_add(i);
            header.push(format!("{addr}"));
            let key = CellKey::new(g.unit, g.fc, addr);
            let v = if is_bit_fc_key(key) {
                self.shared.lock().unwrap().bits.get(&key).map(|b| if *b { 1 } else { 0 }).unwrap_or(-1)
            } else {
                snap.get(&key).map(|x| *x as i32).unwrap_or(-1)
            };
            row.push(format!("{v}"));
        }
        let _ = n;
        let _ = self.logw.write_row(&self.log, &header, &row);
    }
}

impl eframe::App for MbApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut pending: Vec<Cmd> = Vec::new();
        self.shortcuts(ctx, &mut pending);
        self.do_log_tick();
        let snap = self.snapshot(self.view == View::Chart);

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New\tCtrl+N").clicked() {
                        self.project = Project::default();
                        self.active_group = 0;
                        pending.push(Cmd::ClearGroups);
                        for g in &self.project.groups {
                            pending.push(Cmd::AddGroup(g.clone()));
                        }
                        self.restart_worker();
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label(format!("workspace: {}", self.path_buf));
                    if ui.button("Open workspace…\tCtrl+O").clicked() {
                        if let Some(p) = pick_open("mbrs workspace (*.mbw)", "mbw") {
                            self.path_buf = p;
                            self.project_open();
                        }
                        ui.close_menu();
                    }
                    if ui.button("Save workspace…\tCtrl+S").clicked() {
                        if let Some(p) = pick_save("mbrs workspace (*.mbw)", "mbw", "workspace.mbw") {
                            self.path_buf = p;
                            self.project_save();
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Export to CSV…").clicked() {
                        if let Some(p) = pick_save("CSV (*.csv)", "csv", "mbrs-export.csv") {
                            match self.export_csv_to(&p) {
                                Ok(p) => self.status = format!("csv -> {p}"),
                                Err(e) => self.status = format!("csv failed: {e}"),
                            }
                        }
                        ui.close_menu();
                    }
                    if ui.button("Export to Modbus Slave…").clicked() {
                        self.status = "export to Modbus Slave: not implemented".into();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Connection", |ui| {
                    if ui.button("Connect…\tF3").clicked() { self.modal = Modal::Connection; ui.close_menu(); }
                    if ui.button("Disconnect\tF4").clicked() { self.restart_worker(); ui.close_menu(); }
                    if ui.button("Enable").clicked() {
                        for g in self.project.groups.iter_mut() { g.enabled = true; }
                        ui.close_menu();
                    }
                    if ui.button("Disable").clicked() {
                        for g in self.project.groups.iter_mut() { g.enabled = false; }
                        ui.close_menu();
                    }
                    if ui.button("Quick Connect\tF5").clicked() { self.restart_worker(); ui.close_menu(); }
                });
                ui.menu_button("Functions", |ui| {
                    if ui.button("05 Write Single Coil…\tAlt+F5").clicked() { self.modal = Modal::WriteCoil; ui.close_menu(); }
                    if ui.button("06 Write Single Register…\tAlt+F6").clicked() { self.modal = Modal::WriteReg; ui.close_menu(); }
                    if ui.button("08 Diagnostics…").clicked() { self.modal = Modal::Diagnostics; ui.close_menu(); }
                    if ui.button("11 Get Comm Event Counter…").clicked() { self.modal = Modal::CommEvent; ui.close_menu(); }
                    if ui.button("15 Write Multiple Coils…\tAlt+F7").clicked() { self.modal = Modal::WriteMultiCoils; ui.close_menu(); }
                    if ui.button("16 Write Multiple Registers…\tAlt+F8").clicked() { self.modal = Modal::WriteMultiRegs; ui.close_menu(); }
                    if ui.button("17 Report Server ID…").clicked() { self.modal = Modal::ReportId; ui.close_menu(); }
                    if ui.button("22 Mask Write Register…").clicked() { self.modal = Modal::MaskWrite; ui.close_menu(); }
                    if ui.button("23 Read/Write Multiple Registers…").clicked() { self.modal = Modal::ReadWriteMulti; ui.close_menu(); }
                    if ui.button("43/14 Read Device Identification…").clicked() { self.modal = Modal::DeviceId; ui.close_menu(); }
                });
                ui.menu_button("Setup", |ui| {
                    if ui.button("Read/Write Definition…\tF8").clicked() {
                        if let Some(g) = self.active() { self.g_edit = g.clone(); }
                        self.modal = Modal::GroupDef;
                        ui.close_menu();
                    }
                    if ui.button("Set Slave ID for all…\tShift+F8").clicked() { self.modal = Modal::SetSlaveAll; ui.close_menu(); }
                    if ui.button("Set Scan Rate for all…").clicked() { self.modal = Modal::SetScanRateAll; ui.close_menu(); }
                    ui.separator();
                    if ui.button("Log…\tAlt+L").clicked() { self.toggle_log(true); self.modal = Modal::Log; ui.close_menu(); }
                    if ui.button("Logging Off\tAlt+O").clicked() { self.toggle_log(false); ui.close_menu(); }
                    if ui.button("Excel Log Setup…").clicked() { self.modal = Modal::ExcelLog; ui.close_menu(); }
                    ui.separator();
                    if ui.button("Reset Counters\tF12").clicked() {
                        self.shared.lock().unwrap().stats = Default::default();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Address Scan…\tAlt+A").clicked() { self.modal = Modal::AddressScan; self.view = View::Scan; ui.close_menu(); }
                    if ui.button("Slave Scan…").clicked() { self.modal = Modal::SlaveScan; self.view = View::Scan; ui.close_menu(); }
                    ui.separator();
                    if ui.button("Error Counters…\tF11").clicked() { self.modal = Modal::ErrorCounters; ui.close_menu(); }
                    if ui.button("Advanced…").clicked() { self.modal = Modal::Advanced; ui.close_menu(); }
                });
                ui.menu_button("Display", |ui| {
                    for (lbl, f) in [
                        ("Signed\tAlt+Shift+S", ValueFormat::I16),
                        ("Unsigned\tAlt+Shift+U", ValueFormat::U16),
                        ("Hex\tAlt+Shift+H", ValueFormat::Hex16),
                        ("ASCII - Hex\tAlt+Shift+A", ValueFormat::Ascii16),
                        ("Binary\tAlt+Shift+B", ValueFormat::Bin16),
                    ] {
                        if ui.button(lbl).clicked() { self.set_format_all_selected(f, &mut pending); ui.close_menu(); }
                    }
                    ui.separator();
                    if ui.button("Show Binary Names\tAlt+Shift+N").clicked() {
                        self.show_bits = !self.show_bits;
                        ui.close_menu();
                    }
                    ui.separator();
                    for o in [WordOrder::BigEndian, WordOrder::LittleEndian, WordOrder::BigEndianByteSwap, WordOrder::LittleEndianByteSwap] {
                        if ui.button(o.label()).clicked() {
                            self.set_format_all_selected(ValueFormat::U32(o), &mut pending);
                            ui.close_menu();
                        }
                    }
                    ui.menu_button("32/64-bit types", |ui| {
                        for o in [WordOrder::BigEndian, WordOrder::LittleEndian, WordOrder::BigEndianByteSwap, WordOrder::LittleEndianByteSwap] {
                            for f in [ValueFormat::I32(o), ValueFormat::U32(o), ValueFormat::F32(o), ValueFormat::I64(o), ValueFormat::U64(o), ValueFormat::F64(o)] {
                                if ui.button(f.label()).clicked() { self.set_format_all_selected(f, &mut pending); ui.close_menu(); }
                            }
                        }
                    });
                    ui.separator();
                    if ui.button("Real Time Charting…\tAlt+R").clicked() { self.view = View::Chart; ui.close_menu(); }
                    if ui.button("Colors…\tAlt+Shift+C").clicked() { self.modal = Modal::Colors; ui.close_menu(); }
                    if ui.button("Font…\tAlt+Shift+F").clicked() { self.modal = Modal::Font; ui.close_menu(); }
                    if ui.button("Scaling…\tCtrl+Shift+S").clicked() { self.modal = Modal::Scaling; ui.close_menu(); }
                    if ui.button("Value Names…\tCtrl+Shift+V").clicked() { self.modal = Modal::Names; ui.close_menu(); }
                    if ui.button("Binary Names…").clicked() { self.modal = Modal::BinaryNames; ui.close_menu(); }
                });
                ui.menu_button("View", |ui| {
                    for (lbl, v) in [("Register grid", View::Grid), ("SCADA dashboard", View::Scada), ("Strip chart", View::Chart), ("Communication traffic", View::Traffic), ("Scan result", View::Scan), ("Test center", View::Test)] {
                        if ui.selectable_label(self.view == v, lbl).clicked() { self.view = v; ui.close_menu(); }
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Test Center…").clicked() { self.view = View::Test; ui.close_menu(); }
                    if ui.button("Strings…").clicked() { self.modal = Modal::Strings; ui.close_menu(); }
                    if ui.button("Modbus/TCP Security…").clicked() { self.modal = Modal::Security; ui.close_menu(); }
                    if ui.button("About mbrs").clicked() { self.modal = Modal::About; ui.close_menu(); }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (txt, col) = match &snap.conn {
                        ConnState::Connected => ("● connected", Color32::from_rgb(80, 220, 120)),
                        ConnState::Connecting => ("● connecting", Color32::from_rgb(230, 200, 60)),
                        ConnState::Error(e) => { ui.colored_label(Color32::from_rgb(230, 90, 90), format!("● {e}")); ui.label("|"); ("", Color32::GRAY) }
                        ConnState::Disconnected => ("● disconnected", Color32::from_rgb(180, 180, 180)),
                    };
                    if !txt.is_empty() { ui.colored_label(col, txt); }
                });
            });
        });

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("mbrs");
                ui.separator();
                for (lbl, v) in [
                    ("Grid", View::Grid),
                    ("SCADA", View::Scada),
                    ("Chart", View::Chart),
                    ("Traffic", View::Traffic),
                    ("Scan", View::Scan),
                    ("Test", View::Test),
                ] {
                    if ui.selectable_label(self.view == v, lbl).clicked() {
                        self.view = v;
                    }
                }
                ui.separator();
                if ui.button("Connect").clicked() {
                    self.restart_worker();
                }
                if ui.button("Definition").clicked() {
                    if let Some(g) = self.active() {
                        self.g_edit = g.clone();
                    }
                    self.modal = Modal::GroupDef;
                }
                if ui.button("Colors").clicked() {
                    self.modal = Modal::Colors;
                }
                if ui.button("Scaling").clicked() {
                    self.modal = Modal::Scaling;
                }
                if ui.button("Log").clicked() {
                    self.toggle_log(!self.log.enabled);
                }
            });
        });

        egui::SidePanel::left("side").resizable(true).default_width(330.0).show(ctx, |ui| {
            ui.heading("mbrs · Modbus Studio");
            ui.label(format!("{}  {}:{}", self.project.conn.mode.label(), self.project.conn.host, self.project.conn.port));
            ui.label(format!("unit {}   timeout {} ms", self.project.conn.unit, self.project.conn.timeout_ms));
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("＋ group").clicked() {
                    let id = self.project.groups.iter().map(|g| g.id).max().unwrap_or(0) + 1;
                    let g = PollGroup::new(id);
                    let gc = g.clone();
                    self.project.groups.push(g);
                    self.active_group = self.project.groups.len() - 1;
                    pending.push(Cmd::AddGroup(gc));
                }
                if ui.button("Reconnect").clicked() { self.restart_worker(); }
            });
            ui.separator();
            let mut remove: Option<usize> = None;
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for (i, g) in self.project.groups.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(self.active_group == i, format!("{} [{}]", g.name, g.id)).clicked() {
                            self.active_group = i;
                        }
                        if ui.checkbox(&mut g.enabled, "").changed() {
                            let gc = g.clone();
                            pending.push(Cmd::UpdateGroup(gc));
                        }
                        if ui.small_button("x").clicked() { remove = Some(i); }
                    });
                }
            });
            if let Some(i) = remove {
                let g = self.project.groups.remove(i);
                pending.push(Cmd::RemoveGroup(g.id));
                if self.active_group >= self.project.groups.len() && !self.project.groups.is_empty() {
                    self.active_group = self.project.groups.len() - 1;
                }
            }
            ui.separator();
            ui.label(format!("ok {}   err {}   timeouts {}", snap.stats.ok, snap.stats.err, snap.stats.timeouts));
            ui.label(format!("last {:.1} ms", snap.stats.last_ms));
            if !snap.stats.last_error.is_empty() {
                ui.colored_label(Color32::from_rgb(230, 120, 120), &snap.stats.last_error);
            }
            if self.log.enabled {
                ui.colored_label(Color32::from_rgb(120, 220, 140), format!("logging → {}", self.log.path));
            }
            ui.separator();
            ui.label(format!("Status: {}", self.status));
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let col = match &snap.conn {
                    ConnState::Connected => Color32::from_rgb(80, 220, 120),
                    ConnState::Connecting => Color32::from_rgb(230, 200, 60),
                    ConnState::Error(_) => Color32::from_rgb(230, 90, 90),
                    ConnState::Disconnected => Color32::from_rgb(170, 170, 170),
                };
                ui.colored_label(col, "●");
                ui.label(format!("{}:{}", self.project.conn.host, self.project.conn.port));
                ui.separator();
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!(
                        "ok {}   err {}   timeouts {}   {:.1} ms",
                        snap.stats.ok, snap.stats.err, snap.stats.timeouts, snap.stats.last_ms
                    ));
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Grid => self.draw_grid(ui, &snap, &mut pending),
            View::Scada => self.draw_scada(ui, &snap),
            View::Chart => self.draw_chart(ui, &snap),
            View::Traffic => draw_traffic(ui, &self.traffic()),
            View::Scan => draw_scan(ui, &snap),
            View::Test => self.draw_test(ui, &snap, &mut pending),
        });

        self.draw_modal(ctx, &snap, &mut pending);

        for c in pending {
            self.send(c);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(120));
    }
}

impl MbApp {
    fn shortcuts(&mut self, ctx: &egui::Context, pending: &mut Vec<Cmd>) {
        let mut keys: Vec<(Key, Modifiers)> = Vec::new();
        ctx.input(|i| {
            for ev in &i.events {
                if let egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } = ev {
                    keys.push((*key, *modifiers));
                }
            }
        });
        for (k, m) in keys {
            let (c, s, a) = (m.ctrl, m.shift, m.alt);
            match (k, c, s, a) {
                (Key::F3, false, false, false) => self.modal = Modal::Connection,
                (Key::F4, false, false, false) | (Key::F5, false, false, false) => self.restart_worker(),
                (Key::F8, false, false, false) => {
                    if let Some(g) = self.active() { self.g_edit = g.clone(); }
                    self.modal = Modal::GroupDef;
                }
                (Key::F8, false, true, false) => self.modal = Modal::SetSlaveAll,
                (Key::F11, false, false, false) => self.modal = Modal::ErrorCounters,
                (Key::F12, false, false, false) => { self.shared.lock().unwrap().stats = Default::default(); }
                (Key::F12, false, true, false) => { self.shared.lock().unwrap().stats = Default::default(); }
                (Key::F5, false, false, true) => self.modal = Modal::WriteCoil,
                (Key::F6, false, false, true) => self.modal = Modal::WriteReg,
                (Key::F7, false, false, true) => self.modal = Modal::WriteMultiCoils,
                (Key::F8, false, false, true) => self.modal = Modal::WriteMultiRegs,
                (Key::A, false, false, true) => { self.modal = Modal::AddressScan; self.view = View::Scan; }
                (Key::R, false, false, true) => self.view = View::Chart,
                (Key::L, false, false, true) => self.toggle_log(true),
                (Key::O, false, false, true) => self.toggle_log(false),
                (Key::C, false, true, true) => self.modal = Modal::Colors,
                (Key::F, false, true, true) => self.modal = Modal::Font,
                (Key::N, false, true, true) => self.show_bits = !self.show_bits,
                (Key::S, true, true, false) => self.modal = Modal::Scaling,
                (Key::V, true, true, false) => self.modal = Modal::Names,
                (Key::S, false, true, true) => self.set_format_all_selected(ValueFormat::I16, pending),
                (Key::U, false, true, true) => self.set_format_all_selected(ValueFormat::U16, pending),
                (Key::H, false, true, true) => self.set_format_all_selected(ValueFormat::Hex16, pending),
                (Key::A, false, true, true) => self.set_format_all_selected(ValueFormat::Ascii16, pending),
                (Key::B, false, true, true) => self.set_format_all_selected(ValueFormat::Bin16, pending),
                (Key::S, true, false, false) => self.project_save(),
                (Key::O, true, false, false) => self.project_open(),
                (Key::N, true, false, false) => {
                    self.project = Project::default();
                    self.restart_worker();
                }
                _ => {}
            }
        }
    }

    fn draw_grid(&mut self, ui: &mut egui::Ui, snap: &Snap, pending: &mut Vec<Cmd>) {
        let Some(g) = self.project.groups.get(self.active_group).cloned() else {
            ui.label("No poll group. Use ＋ group.");
            return;
        };
        ui.horizontal(|ui| {
            ui.strong(&g.name);
            ui.separator();
            ui.label(fc_label(g.fc));
            ui.separator();
            ui.label(format!("start {}  count {}  scan {} ms", g.start, g.count, g.scan_ms));
            ui.separator();
            ui.label(g.format.label());
            if ui.button("Edit…").clicked() {
                self.g_edit = g.clone();
                self.modal = Modal::GroupDef;
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if let Some(k) = self.selected {
                ui.label(format!("sel {} @ {}", fc_label(k.fc), k.addr));
                ui.text_edit_singleline(&mut self.write_buf);
                if ui.button("Write").clicked() {
                    if is_bit_fc_key(k) {
                        let on = matches!(self.write_buf.trim(), "1" | "true" | "on" | "ON");
                        pending.push(Cmd::WriteCoil { unit: k.unit, addr: k.addr, on });
                    } else if let Some(regs) = g.format.parse_to_regs(&self.write_buf) {
                        if regs.len() == 1 {
                            pending.push(Cmd::WriteReg { unit: k.unit, addr: k.addr, value: regs[0] });
                        } else {
                            pending.push(Cmd::WriteRegs { unit: k.unit, addr: k.addr, values: regs });
                        }
                    } else {
                        self.status = "cannot parse value".into();
                    }
                }
            } else {
                ui.label("click a cell to select it");
            }
        });
        ui.separator();
        let n = g.format.regs_needed();
        let row_h = 22.0;
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Frame::none()
                .fill(Color32::from_rgb(33, 36, 43))
                .inner_margin(egui::Margin::symmetric(6.0, 3.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong("addr");
                        ui.add_space(48.0);
                        ui.strong("raw");
                        ui.add_space(96.0);
                        ui.strong("value");
                    });
                });
            for i in 0..g.count {
                let addr = g.start.wrapping_add(i);
                let key = CellKey::new(g.unit, g.fc, addr);
                let stripe = if i % 2 == 0 { Color32::from_rgb(25, 27, 33) } else { Color32::from_rgb(21, 23, 28) };
                egui::Frame::none()
                    .fill(stripe)
                    .inner_margin(egui::Margin::symmetric(6.0, 1.0))
                    .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let disp = if self.plc_addr { 1 + addr as u32 } else { addr as u32 };
                    ui.monospace(format!("{disp:6}"));
                    if is_bit_fc_key(key) {
                        let on = snap.bits.get(&key).copied().unwrap_or(false);
                        let val = if on { 1.0 } else { 0.0 };
                        let (bg, fg) = g.color.color_for(val, if on { 1 } else { 0 });
                        let resp = draw_cell(ui, 90.0, row_h, if on { "1" } else { "0" }, bg, fg, self.selected == Some(key));
                        if resp.clicked() {
                            self.selected = Some(key);
                            self.write_buf = if on { "1".into() } else { "0".into() };
                        }
                        let nm = self.names.bit(0).cloned().unwrap_or_default();
                        let _ = nm;
                    } else {
                        let raw = snap.words.get(&key).copied();
                        let raws = raw.map(|v| format!("{v:04X}")).unwrap_or_else(|| "----".into());
                        draw_cell_static(ui, 70.0, row_h, &raws, [40, 44, 52], [180, 190, 200]);
                        if self.show_bits {
                            if let Some(v) = raw {
                                for b in (0..16).rev() {
                                    let on = (v >> b) & 1 == 1;
                                    let nm = self.names.bit(b).cloned().unwrap_or_else(|| format!("b{b}"));
                                    draw_cell_static(ui, 54.0, row_h, &format!("{}", if on { 1 } else { 0 }), if on { [40, 110, 60] } else { [50, 40, 40] }, [220, 220, 220]);
                                    let _ = nm;
                                }
                            }
                        } else if i % n as u16 == 0 {
                            let regs: Vec<u16> = (0..n as u16)
                                .filter_map(|o| snap.words.get(&CellKey::new(g.unit, g.fc, addr.wrapping_add(o))).copied())
                                .collect();
                            if regs.len() == n {
                                let numeric = g.format.numeric(&regs);
                                let scaled = g.scale.apply(numeric);
                                let shown = if g.scale.enabled { format!("{:.*}", g.scale.decimals as usize, scaled) } else { g.format.format(&regs) };
                                let (bg, fg) = g.color.color_for(scaled, regs[0] as u64);
                                let resp = draw_cell(ui, 190.0, row_h, &shown, bg, fg, self.selected == Some(key));
                                if resp.clicked() {
                                    self.selected = Some(key);
                                    self.write_buf = g.format.format(&regs);
                                }
                                if let Some(t) = self.names.text(regs[0]) {
                                    ui.label(t);
                                }
                            } else {
                                draw_cell_static(ui, 190.0, row_h, "…", [40, 44, 52], [120, 120, 120]);
                            }
                        } else {
                            draw_cell_static(ui, 190.0, row_h, "(cont)", [34, 36, 42], [90, 90, 100]);
                        }
                    }
                });
                    });
            }
        });
    }

    fn draw_scada(&mut self, ui: &mut egui::Ui, snap: &Snap) {
        ui.horizontal(|ui| {
            ui.strong("SCADA dashboard");
            if ui.button("＋ tile from group").clicked() {
                if let Some(g) = self.active() {
                    let id = self.project.tiles.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                    let mut t = Tile::new(id, g.start);
                    t.unit = g.unit; t.fc = g.fc; t.format = g.format; t.label = g.name.clone();
                    t.lo = g.color.ramp_lo; t.hi = g.color.ramp_hi; t.color = g.color.clone();
                    self.project.tiles.push(t);
                    autolayout(&mut self.project.tiles, 4);
                }
            }
            if ui.button("Auto-layout").clicked() { autolayout(&mut self.project.tiles, 4); }
            if let Some(i) = self.sel_tile {
                if ui.button("Delete tile").clicked() { self.project.tiles.remove(i); self.sel_tile = None; }
            }
        });
        ui.separator();
        let size = ui.available_size();
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::click());
        let p = ui.painter();
        p.rect_filled(rect, 4.0, Color32::from_rgb(12, 12, 16));
        for (i, t) in self.project.tiles.iter().enumerate() {
            let tr = egui::Rect::from_min_size(rect.min + egui::vec2(t.x * rect.width(), t.y * rect.height()), egui::vec2(t.w * rect.width(), t.h * rect.height()));
            let key = CellKey::new(t.unit, t.fc, t.addr);
            let raw = if is_bit_fc_key(key) {
                snap.bits.get(&key).copied().map(|b| if b { 1u64 } else { 0 }).unwrap_or(0)
            } else {
                snap.words.get(&key).copied().unwrap_or(0) as u64
            };
            let regs: Vec<u16> = (0..t.format.regs_needed() as u16)
                .filter_map(|o| snap.words.get(&CellKey::new(t.unit, t.fc, t.addr.wrapping_add(o))).copied())
                .collect();
            let numeric = if is_bit_fc_key(key) { raw as f64 } else if regs.len() == t.format.regs_needed() { t.format.numeric(&regs) } else { f64::NAN };
            let (bg, fg) = t.color.color_for(numeric, raw);
            let bgc = Color32::from_rgb(bg[0], bg[1], bg[2]);
            let fgc = Color32::from_rgb(fg[0], fg[1], fg[2]);
            p.rect_filled(tr, 6.0, bgc);
            p.rect_stroke(tr, 6.0, Stroke::new(if self.sel_tile == Some(i) { 2.5 } else { 1.0 }, if self.sel_tile == Some(i) { Color32::from_rgb(90, 160, 255) } else { Color32::from_rgb(60, 60, 70) }));
            let title = if t.label.is_empty() { format!("REG {}", t.addr) } else { t.label.clone() };
            p.text(tr.left_top() + egui::vec2(8.0, 6.0), Align2::LEFT_TOP, &title, FontId::proportional(13.0), fgc);
            let txt = if regs.len() == t.format.regs_needed() { t.format.format(&regs) } else if is_bit_fc_key(key) { if raw == 1 { "ON".into() } else { "OFF".into() } } else { "----".into() };
            match t.kind {
                TileKind::Value => { p.text(tr.center() + egui::vec2(0.0, 6.0), Align2::CENTER_CENTER, txt, FontId::monospace(26.0), fgc); }
                TileKind::Lamp => {
                    let r = tr.height().min(tr.width()) * 0.22;
                    p.circle_filled(tr.center() + egui::vec2(0.0, 6.0), r, bgc);
                    p.circle_stroke(tr.center() + egui::vec2(0.0, 6.0), r, Stroke::new(2.0, fgc));
                    p.text(tr.center() + egui::vec2(0.0, r + 18.0), Align2::CENTER_CENTER, txt, FontId::proportional(12.0), fgc);
                }
                TileKind::Bar | TileKind::Gauge => {
                    let inner = egui::Rect::from_min_max(tr.min + egui::vec2(10.0, tr.height() * 0.55), tr.max - egui::vec2(10.0, 10.0));
                    p.rect_filled(inner, 3.0, Color32::from_rgb(20, 20, 26));
                    let span = (t.hi - t.lo).abs();
                    let frac = if span < 1e-9 { 0.0 } else { ((numeric - t.lo) / span).clamp(0.0, 1.0) } as f32;
                    let fill = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width() * frac, inner.height()));
                    p.rect_filled(fill, 3.0, bgc);
                    p.rect_stroke(inner, 3.0, Stroke::new(1.0, Color32::from_rgb(70, 70, 80)));
                    p.text(tr.center() + egui::vec2(0.0, 6.0), Align2::CENTER_CENTER, txt, FontId::monospace(20.0), fgc);
                }
            }
        }
        if self.project.tiles.is_empty() {
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "add tiles from the active group", FontId::proportional(16.0), Color32::GRAY);
        }
    }

    fn draw_chart(&self, ui: &mut egui::Ui, snap: &Snap) {
        let key = self.selected.or_else(|| self.active().map(|g| CellKey::new(g.unit, g.fc, g.start)));
        let Some(key) = key else { ui.label("no series"); return };
        let hist = snap.history.get(&key).cloned().unwrap_or_default();
        let label = format!("{} @ {}", fc_label(key.fc), key.addr);
        let size = ui.available_size();
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, 4.0, Color32::from_rgb(18, 18, 22));
        if hist.len() < 2 {
            p.text(rect.center(), Align2::CENTER_CENTER, "waiting for data…", FontId::proportional(14.0), Color32::GRAY);
            return;
        }
        let (mn, mx) = if self.series_auto {
            (hist.iter().cloned().fold(f64::INFINITY, f64::min), hist.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
        } else {
            (self.series_ymin, self.series_ymax)
        };
        let span = if (mx - mn).abs() < 1e-9 { 1.0 } else { mx - mn };
        let n = hist.len();
        let start = n.saturating_sub(self.series_points);
        let pts: Vec<egui::Pos2> = hist.iter().skip(start).enumerate().map(|(i, v)| {
            let x = rect.left() + rect.width() * (i as f32 / ((n - start).max(2) - 1) as f32);
            let y = rect.bottom() - rect.height() * (((v - mn) / span) as f32);
            egui::pos2(x, y)
        }).collect();
        for w in pts.windows(2) {
            p.line_segment([w[0], w[1]], Stroke::new(1.5, Color32::from_rgb(90, 200, 255)));
        }
        p.text(rect.left_top() + egui::vec2(8.0, 6.0), Align2::LEFT_TOP, format!("{label}  min={mn:.3} max={mx:.3} last={:.3} n={n}", hist.back().copied().unwrap_or(0.0)), FontId::monospace(12.0), Color32::LIGHT_GRAY);
    }

    fn draw_test(&mut self, ui: &mut egui::Ui, snap: &Snap, pending: &mut Vec<Cmd>) {
        ui.strong("Test Center");
        ui.separator();
        egui::Grid::new("test").show(ui, |ui| {
            ui.label("Slave/Unit"); ui.add(egui::DragValue::new(&mut self.test_unit).range(0..=255)); ui.end_row();
            ui.label("Raw PDU (hex)"); ui.text_edit_singleline(&mut self.test_pdu); ui.end_row();
            ui.label("Comment"); ui.text_edit_singleline(&mut self.test_comment); ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.button("Send").clicked() {
                if let Some(pdu) = parse_hex(&self.test_pdu) {
                    pending.push(Cmd::Raw { unit: self.test_unit, pdu });
                } else {
                    self.status = "bad hex PDU".into();
                }
            }
            if ui.button("Read Coils 0..7").clicked() {
                pending.push(Cmd::Raw { unit: self.test_unit, pdu: vec![0x01, 0, 0, 0, 8] });
            }
            if ui.button("Read Holding 0..9").clicked() {
                pending.push(Cmd::Raw { unit: self.test_unit, pdu: vec![0x03, 0, 0, 0, 10] });
            }
        });
        ui.separator();
        ui.label("Response:");
        ui.monospace(if snap.last_response.is_empty() { "(none)".into() } else { snap.last_response.clone() });
    }

    fn traffic(&self) -> Vec<crate::store::TrafficEntry> {
        self.shared.lock().unwrap().traffic.iter().cloned().collect()
    }

    fn export_csv_to(&self, path: &str) -> anyhow::Result<String> {
        let s = self.shared.lock().unwrap();
        let mut out = String::from("unit,fc,addr,value\n");
        let mut keys: Vec<&CellKey> = s.words.keys().collect();
        keys.sort_by_key(|k| (k.unit, k.fc, k.addr));
        for k in keys {
            out.push_str(&format!("{},{},0x{:04X},{}\n", k.unit, k.fc, k.addr, s.words[k]));
        }
        std::fs::write(path, out)?;
        Ok(path.to_string())
    }

    fn export_csv(&self) -> anyhow::Result<String> {
        self.export_csv_to(&format!("{}.csv", self.path_buf.trim_end_matches(".mbw")))
    }
}

// --------------------------- modal dialogs ---------------------------------

impl MbApp {
    fn draw_modal(&mut self, ctx: &egui::Context, snap: &Snap, pending: &mut Vec<Cmd>) {
        if self.modal == Modal::None {
            return;
        }
        let mut open = true;
        if self.modal == Modal::Connection {
            self.dlg_connection(ctx, &mut open);
        } else {
            let title = self.modal_title();
            egui::Window::new(title)
                .open(&mut open)
                .collapsible(true)
                .resizable(true)
                .default_width(560.0)
                .show(ctx, |ui| {
                    self.modal_body(ui, snap, pending);
                });
        }
        if !open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.modal = Modal::None;
        }
    }

    fn modal_title(&self) -> &'static str {
        match self.modal {
            Modal::None => "",
            Modal::Connection => "Connection Setup",
            Modal::GroupDef => "Read/Write Definition",
            Modal::SetSlaveAll => "Set Slave ID for all Windows",
            Modal::SetScanRateAll => "Set Scan Rate for all Windows",
            Modal::Colors => "Cell Colors",
            Modal::Scaling => "Scaling",
            Modal::Font => "Font",
            Modal::Names => "Value Names",
            Modal::BinaryNames => "Binary Names",
            Modal::Log => "Log Setup",
            Modal::ExcelLog => "Excel Log Setup",
            Modal::Series => "Series Settings",
            Modal::ErrorCounters => "Error Counters",
            Modal::Advanced => "Advanced",
            Modal::Security => "Modbus/TCP Security",
            Modal::About => "About",
            Modal::WriteCoil => "05 (0x05) Write Single Coil",
            Modal::WriteReg => "06 (0x06) Write Single Register",
            Modal::WriteRegBinary => "Write Single Register Binary",
            Modal::WriteInteger => "Write Integer",
            Modal::WriteFloat => "Write Float",
            Modal::WriteMultiCoils => "15 (0x0F) Write Multiple Coils",
            Modal::WriteMultiRegs => "16 (0x10) Write Multiple Registers",
            Modal::MaskWrite => "22 (0x16) Mask Write Register",
            Modal::ReadWriteMulti => "23 (0x17) Read/Write Multiple Registers",
            Modal::Diagnostics => "08 (0x08) Diagnostics",
            Modal::CommEvent => "11 (0x0B) Get Comm Event Counter",
            Modal::ReportId => "17 (0x11) Report Server ID",
            Modal::DeviceId => "43 / 14 (0x2B / 0x0E) Read Device Identification",
            Modal::AddressScan => "Address Scan",
            Modal::SlaveScan => "Slave Scan",
            Modal::TestCenter => "Test Center",
            Modal::Strings => "Strings",
        }
    }

    fn modal_body(&mut self, ui: &mut egui::Ui, snap: &Snap, pending: &mut Vec<Cmd>) {
        match self.modal {
            Modal::GroupDef => self.body_groupdef(ui, pending),
            Modal::Colors => self.body_colors(ui, pending),
            Modal::Scaling => self.body_scaling(ui, pending),
            Modal::Names => self.body_names(ui),
            Modal::BinaryNames => self.body_binary_names(ui),
            Modal::Log | Modal::ExcelLog => self.body_log(ui),
            Modal::Series => self.body_series(ui),
            Modal::ErrorCounters => {
                egui::Grid::new("ec").show(ui, |ui| {
                    ui.label("Transmit (ok)"); ui.monospace(format!("{}", snap.stats.ok)); ui.end_row();
                    ui.label("Errors"); ui.monospace(format!("{}", snap.stats.err)); ui.end_row();
                    ui.label("Timeouts"); ui.monospace(format!("{}", snap.stats.timeouts)); ui.end_row();
                    ui.label("Last response"); ui.monospace(format!("{:.1} ms", snap.stats.last_ms)); ui.end_row();
                    ui.label("Last error"); ui.monospace(&snap.stats.last_error); ui.end_row();
                });
            }
            Modal::About => {
                ui.label("mbrs — Modbus Studio (Rust)");
                ui.label("Independent reimplementation; no Modbus Poll code used.");
                ui.label("Reverse-engineered surface: 44 dialogs, 3 menus, 41 string bundles.");
            }
            Modal::Advanced => { ui.checkbox(&mut self.plc_addr, "Display addresses as PLC addresses (Base 1)"); }
            Modal::Security => {
                ui.label("Modbus/TCP Security (TLS) — transport seam present, TLS backend not wired.");
                ui.label("Fail-closed: connect with Modbus/TCP and a TLS terminator in front.");
            }
            Modal::SetSlaveAll => {
                ui.horizontal(|ui| { ui.label("Slave ID (all groups)"); ui.add(egui::DragValue::new(&mut self.all_unit).range(0..=255)); });
                if ui.button("Apply").clicked() {
                    for g in self.project.groups.iter_mut() { g.unit = self.all_unit; pending.push(Cmd::UpdateGroup(g.clone())); }
                    self.modal = Modal::None;
                }
            }
            Modal::SetScanRateAll => {
                ui.horizontal(|ui| { ui.label("Scan rate ms (all groups)"); ui.add(egui::DragValue::new(&mut self.all_scan).speed(10.0).range(0..=3_600_000)); });
                if ui.button("Apply").clicked() {
                    for g in self.project.groups.iter_mut() { g.scan_ms = self.all_scan; pending.push(Cmd::UpdateGroup(g.clone())); }
                    self.modal = Modal::None;
                }
            }
            Modal::Font => { ui.label("Per-cell font (LOGFONT in the original). Uses the app font here."); }
            Modal::TestCenter => self.draw_test(ui, snap, pending),
            Modal::Strings => {
                ui.label("Test Center string file (comments) — ASCII example: 01 03 00 00 00 02");
                ui.text_edit_multiline(&mut self.test_ascii);
            }
            Modal::WriteCoil => self.body_write_bits(ui, pending, false),
            Modal::WriteMultiCoils => self.body_write_bits(ui, pending, true),
            Modal::WriteReg | Modal::WriteInteger | Modal::WriteRegBinary | Modal::WriteFloat => self.body_write_reg(ui, pending),
            Modal::WriteMultiRegs => self.body_write_regs(ui, pending),
            Modal::MaskWrite => self.body_mask(ui, pending),
            Modal::ReadWriteMulti => self.body_rw_multi(ui, pending),
            Modal::Diagnostics => self.body_diag(ui, snap, pending),
            Modal::CommEvent => self.body_comm_event(ui, snap, pending),
            Modal::ReportId => self.body_report_id(ui, snap, pending),
            Modal::DeviceId => self.body_device_id(ui, snap, pending),
            Modal::AddressScan => self.body_address_scan(ui, snap, pending),
            Modal::SlaveScan => self.body_slave_scan(ui, snap, pending),
            _ => {}
        }
    }

    fn dlg_connection(&mut self, ctx: &egui::Context, open: &mut bool) {
        egui::Window::new("Connection Setup").open(open).default_width(420.0).show(ctx, |ui| {
            let c = &mut self.project.conn;
            egui::ComboBox::from_id_source("mode").selected_text(c.mode.label()).show_ui(ui, |ui| {
                for m in [Mode::Tcp, Mode::Rtu, Mode::Ascii, Mode::RtuOverTcp, Mode::AsciiOverTcp, Mode::Udp] {
                    ui.selectable_value(&mut c.mode, m, m.label());
                }
            });
            if c.mode.is_serial() {
                egui::Grid::new("serial").show(ui, |ui| {
                    ui.label("Serial port"); ui.text_edit_singleline(&mut c.serial_port); ui.end_row();
                    ui.label("Baud"); ui.add(egui::DragValue::new(&mut c.baud).speed(50.0)); ui.end_row();
                    ui.label("Data bits"); ui.add(egui::DragValue::new(&mut c.data_bits).range(5..=8)); ui.end_row();
                    ui.label("Parity"); ui.horizontal(|ui| { ui.selectable_value(&mut c.parity, 'N', "None"); ui.selectable_value(&mut c.parity, 'E', "Even"); ui.selectable_value(&mut c.parity, 'O', "Odd"); }); ui.end_row();
                    ui.label("Stop bits"); ui.add(egui::DragValue::new(&mut c.stop_bits).range(1..=2)); ui.end_row();
                });
            } else {
                egui::Grid::new("tcp").show(ui, |ui| {
                    ui.label("Host"); ui.text_edit_singleline(&mut c.host); ui.end_row();
                    ui.label("Port"); ui.add(egui::DragValue::new(&mut c.port).range(1..=65535)); ui.end_row();
                });
            }
            ui.horizontal(|ui| {
                ui.label("Unit id"); ui.add(egui::DragValue::new(&mut c.unit).range(0..=255));
                ui.label("Timeout ms"); ui.add(egui::DragValue::new(&mut c.timeout_ms).speed(10.0).range(10..=60000));
            });
            if ui.button("Apply & reconnect").clicked() {
                self.restart_worker();
            }
        });
    }

    fn body_groupdef(&mut self, ui: &mut egui::Ui, pending: &mut Vec<Cmd>) {
        let g = &mut self.g_edit;
        egui::Grid::new("gd").show(ui, |ui| {
            ui.label("Name"); ui.text_edit_singleline(&mut g.name); ui.end_row();
            ui.label("Slave ID"); ui.add(egui::DragValue::new(&mut g.unit).range(0..=255)); ui.end_row();
            ui.label("Function");
            egui::ComboBox::from_id_source("gdfc").selected_text(fc_label(g.fc)).show_ui(ui, |ui| {
                for fc in [FC_READ_COILS, FC_READ_DISCRETE, FC_READ_HOLDING, FC_READ_INPUT] {
                    ui.selectable_value(&mut g.fc, fc, fc_label(fc));
                }
            });
            ui.end_row();
            ui.label("Address"); ui.add(egui::DragValue::new(&mut g.start).range(0..=65535)); ui.end_row();
            ui.label("Quantity"); ui.add(egui::DragValue::new(&mut g.count).range(1..=2000)); ui.end_row();
            ui.label("Scan rate (ms)"); ui.add(egui::DragValue::new(&mut g.scan_ms).speed(10.0).range(0..=3_600_000)); ui.end_row();
            ui.label("Format");
            egui::ComboBox::from_id_source("gdfmt").selected_text(g.format.label()).show_ui(ui, |ui| {
                for f in all_formats() { ui.selectable_value(&mut g.format, f, f.label()); }
            });
            ui.end_row();
            ui.checkbox(&mut g.enabled, "Enable");
            ui.checkbox(&mut g.chart, "Chart this series");
            ui.end_row();
        });
        ui.separator();
        if ui.button("Apply").clicked() {
            let gc = self.g_edit.clone();
            if let Some(slot) = self.project.groups.get_mut(self.active_group) { *slot = gc.clone(); }
            pending.push(Cmd::UpdateGroup(gc));
            self.modal = Modal::None;
        }
    }

    fn body_colors(&mut self, ui: &mut egui::Ui, pending: &mut Vec<Cmd>) {
        let Some(g) = self.project.groups.get(self.active_group).cloned() else { return };
        let mut map = g.color.clone();
        ui.horizontal(|ui| { ui.label("Mode:"); for m in ColorMode::ALL { ui.selectable_value(&mut map.mode, *m, m.label()); } });
        if map.mode == ColorMode::Rules {
            ui.horizontal(|ui| { ui.label("Normal"); color_row(ui, "bg", &mut map.normal_bg); color_row(ui, "fg", &mut map.normal_fg); ui.checkbox(&mut map.auto_fg, "auto fg"); });
            for (i, r) in map.rules.iter_mut().enumerate() {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut r.enabled, "");
                    ui.label(format!("Rule {}", i + 1));
                    egui::ComboBox::from_id_source(format!("op{i}")).selected_text(r.op.label()).show_ui(ui, |ui| {
                        for op in ColorOp::ALL { ui.selectable_value(&mut r.op, *op, op.label()); }
                    });
                    ui.add(egui::DragValue::new(&mut r.value).speed(1.0));
                    if r.op == ColorOp::Range { ui.label(".."); ui.add(egui::DragValue::new(&mut r.value2).speed(1.0)); }
                    ui.text_edit_singleline(&mut r.label);
                });
                ui.horizontal(|ui| { ui.label("bg"); swatches(ui, &mut r.bg); ui.label("fg"); swatches(ui, &mut r.fg); ui.checkbox(&mut r.auto_fg, "auto"); });
                if r.auto_fg { r.fg = contrast(r.bg); }
            }
            if ui.button("＋ add rule").clicked() { map.rules.push(ColorRule::new(ColorOp::Gt, 0, PALETTE32[map.rules.len() % 32])); }
        } else {
            egui::Grid::new("ramp").show(ui, |ui| {
                ui.label("Range low"); ui.add(egui::DragValue::new(&mut map.ramp_lo).speed(1.0)); ui.end_row();
                ui.label("Range high"); ui.add(egui::DragValue::new(&mut map.ramp_hi).speed(1.0)); ui.end_row();
                ui.label("Levels (2..32)"); ui.add(egui::Slider::new(&mut map.levels, 2..=32)); ui.end_row();
                ui.label("Palette"); egui::ComboBox::from_id_source("pal").selected_text(map.palette.label()).show_ui(ui, |ui| {
                    for p in Palette::ALL { ui.selectable_value(&mut map.palette, *p, p.label()); }
                });
                ui.end_row();
            });
            ui.horizontal_wrapped(|ui| {
                for i in 0..map.levels.max(2) {
                    let t = i as f64 / (map.levels.max(2) as f64 - 1.0);
                    let v = map.ramp_lo + t * (map.ramp_hi - map.ramp_lo);
                    let (bg, fg) = map.color_for(v, 0);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 22.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(bg[0], bg[1], bg[2]));
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, format!("{v:.0}"), FontId::monospace(10.0), Color32::from_rgb(fg[0], fg[1], fg[2]));
                }
            });
        }
        ui.separator();
        if ui.button("Apply to active group").clicked() {
            if let Some(slot) = self.project.groups.get_mut(self.active_group) { slot.color = map.clone(); pending.push(Cmd::UpdateGroup(slot.clone())); }
            self.modal = Modal::None;
        }
    }

    fn body_scaling(&mut self, ui: &mut egui::Ui, pending: &mut Vec<Cmd>) {
        let Some(mut g) = self.project.groups.get(self.active_group).cloned() else { return };
        ui.checkbox(&mut g.scale.enabled, "Enable scaling");
        egui::Grid::new("sc").show(ui, |ui| {
            ui.label("X1"); ui.add(egui::DragValue::new(&mut g.scale.x1)); ui.end_row();
            ui.label("Y1"); ui.add(egui::DragValue::new(&mut g.scale.y1)); ui.end_row();
            ui.label("X2"); ui.add(egui::DragValue::new(&mut g.scale.x2)); ui.end_row();
            ui.label("Y2"); ui.add(egui::DragValue::new(&mut g.scale.y2)); ui.end_row();
            ui.label("Decimals"); ui.add(egui::DragValue::new(&mut g.scale.decimals).range(0..=6)); ui.end_row();
        });
        ui.label("Y = m·(X − X1) + Y1");
        if ui.button("Apply").clicked() {
            if let Some(slot) = self.project.groups.get_mut(self.active_group) { slot.scale = g.scale; pending.push(Cmd::UpdateGroup(slot.clone())); }
            self.modal = Modal::None;
        }
    }

    fn body_names(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("value"); ui.add(egui::DragValue::new(&mut self.new_name_key).range(0..=65535));
            ui.label("text"); ui.text_edit_singleline(&mut self.new_name_val);
            if ui.button("Add").clicked() {
                self.names.value.insert(self.new_name_key, self.new_name_val.clone());
                self.new_name_val.clear();
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Import…").clicked() {
                if let Some(p) = pick_open("names (*.txt)", "txt") {
                    if let Ok(s) = std::fs::read_to_string(&p) {
                        self.names.import_txt(&s);
                        self.status = format!("imported {p}");
                    }
                }
            }
            if ui.button("Export…").clicked() {
                if let Some(p) = pick_save("names (*.txt)", "txt", "names.txt") {
                    let _ = std::fs::write(&p, self.names.export_txt());
                    self.status = format!("exported {p}");
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
            let mut del: Option<u16> = None;
            for (k, v) in self.names.value.iter_mut() {
                ui.horizontal(|ui| { ui.monospace(format!("{k:>5}")); ui.text_edit_singleline(v); if ui.small_button("x").clicked() { del = Some(*k); } });
            }
            if let Some(k) = del { self.names.value.remove(&k); }
        });
    }

    fn body_binary_names(&mut self, ui: &mut egui::Ui) {
        ui.label("Bit names (0..15) — shown in the grid when \"Show Binary Names\" is active.");
        egui::Grid::new("bn").show(ui, |ui| {
            for b in 0..16u8 {
                let mut s = self.names.bit(b).cloned().unwrap_or_default();
                ui.label(format!("bit {b}"));
                if ui.text_edit_singleline(&mut s).changed() { self.names.set_bit(b, s); }
                if b % 1 == 0 { ui.end_row(); }
            }
        });
    }

    fn body_log(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("File");
            ui.text_edit_singleline(&mut self.log.path);
            if ui.button("Browse…").clicked() {
                if let Some(p) = pick_save("log (*.csv / *.txt)", "csv", "mbrs-log.csv") {
                    self.log.path = p;
                }
            }
        });
        ui.horizontal(|ui| { ui.label("Format"); for f in [LogFormat::Text, LogFormat::Csv] { ui.selectable_value(&mut self.log.format, f, f.label()); } });
        ui.horizontal(|ui| { ui.label("Policy"); for p in LogPolicy::ALL { ui.selectable_value(&mut self.log.policy, *p, p.label()); } });
        ui.horizontal(|ui| { ui.label("Interval ms"); ui.add(egui::DragValue::new(&mut self.log.interval_ms).speed(10.0).range(10..=3_600_000)); ui.checkbox(&mut self.log.append, "Append"); });
        ui.horizontal(|ui| {
            if ui.button("Start logging").clicked() { self.toggle_log(true); }
            if ui.button("Stop").clicked() { self.toggle_log(false); }
        });
    }

    fn body_series(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.series_auto, "Auto scale Y");
        ui.horizontal(|ui| { ui.label("Y min"); ui.add(egui::DragValue::new(&mut self.series_ymin)); ui.label("Y max"); ui.add(egui::DragValue::new(&mut self.series_ymax)); });
        ui.horizontal(|ui| { ui.label("Points shown"); ui.add(egui::DragValue::new(&mut self.series_points).range(10..=5000)); });
    }

    fn body_write_bits(&mut self, ui: &mut egui::Ui, pending: &mut Vec<Cmd>, multi: bool) {
        ui.horizontal(|ui| { ui.label("Unit"); ui.add(egui::DragValue::new(&mut self.w_unit).range(0..=255)); ui.label("Address"); ui.add(egui::DragValue::new(&mut self.w_addr).range(0..=65535)); });
        if multi { ui.horizontal(|ui| { ui.label("Quantity"); ui.add(egui::DragValue::new(&mut self.w_qty).range(1..=2000)); }); }
        ui.horizontal(|ui| { ui.label("Value (0/1)"); ui.text_edit_singleline(&mut self.w_val); });
        if ui.button("Send").clicked() {
            let on = matches!(self.w_val.trim(), "1" | "true" | "on" | "ON");
            if multi {
                let vals: Vec<bool> = self.w_val.chars().map(|c| c == '1').collect();
                if vals.is_empty() { pending.push(Cmd::WriteCoils { unit: self.w_unit, addr: self.w_addr, values: vec![on; self.w_qty as usize] }); }
                else { pending.push(Cmd::WriteCoils { unit: self.w_unit, addr: self.w_addr, values: vals }); }
            } else {
                pending.push(Cmd::WriteCoil { unit: self.w_unit, addr: self.w_addr, on });
            }
            self.modal = Modal::None;
        }
    }

    fn body_write_reg(&mut self, ui: &mut egui::Ui, pending: &mut Vec<Cmd>) {
        ui.horizontal(|ui| { ui.label("Unit"); ui.add(egui::DragValue::new(&mut self.w_unit).range(0..=255)); ui.label("Address"); ui.add(egui::DragValue::new(&mut self.w_addr).range(0..=65535)); });
        ui.horizontal(|ui| { ui.label("Value"); ui.text_edit_singleline(&mut self.w_val); });
        if ui.button("Send (FC 06)").clicked() {
            if let Some(v) = self.w_val.trim().parse::<i64>().ok().map(|x| x as u16) {
                pending.push(Cmd::WriteReg { unit: self.w_unit, addr: self.w_addr, value: v });
                self.modal = Modal::None;
            } else { self.status = "bad value".into(); }
        }
    }

    fn body_write_regs(&mut self, ui: &mut egui::Ui, pending: &mut Vec<Cmd>) {
        ui.horizontal(|ui| { ui.label("Unit"); ui.add(egui::DragValue::new(&mut self.w_unit).range(0..=255)); ui.label("Address"); ui.add(egui::DragValue::new(&mut self.w_addr).range(0..=65535)); });
        ui.label("Values (comma or space separated)");
        ui.text_edit_singleline(&mut self.w_val);
        if ui.button("Send (FC 16)").clicked() {
            let vals: Vec<u16> = self.w_val.split(|c| c == ',' || c == ' ').filter_map(|t| t.trim().parse::<u16>().ok()).collect();
            if vals.is_empty() { self.status = "no values".into(); }
            else { pending.push(Cmd::WriteRegs { unit: self.w_unit, addr: self.w_addr, values: vals }); self.modal = Modal::None; }
        }
    }

    fn body_mask(&mut self, ui: &mut egui::Ui, pending: &mut Vec<Cmd>) {
        ui.horizontal(|ui| { ui.label("Unit"); ui.add(egui::DragValue::new(&mut self.w_unit).range(0..=255)); ui.label("Address"); ui.add(egui::DragValue::new(&mut self.w_addr).range(0..=65535)); });
        ui.horizontal(|ui| { ui.label("AND mask"); ui.add(egui::DragValue::new(&mut self.w_and).range(0..=65535)); ui.label("OR mask"); ui.add(egui::DragValue::new(&mut self.w_or).range(0..=65535)); });
        if ui.button("Send (FC 22)").clicked() {
            pending.push(Cmd::MaskWrite { unit: self.w_unit, addr: self.w_addr, and_mask: self.w_and, or_mask: self.w_or });
            self.modal = Modal::None;
        }
    }

    fn body_rw_multi(&mut self, ui: &mut egui::Ui, pending: &mut Vec<Cmd>) {
        ui.horizontal(|ui| { ui.label("Unit"); ui.add(egui::DragValue::new(&mut self.w_unit).range(0..=255)); });
        ui.horizontal(|ui| { ui.label("Read addr"); ui.add(egui::DragValue::new(&mut self.w_addr).range(0..=65535)); ui.label("Qty"); ui.add(egui::DragValue::new(&mut self.w_qty).range(1..=125)); });
        ui.label("Write values (FC 23)");
        ui.text_edit_singleline(&mut self.w_val);
        if ui.button("Send").clicked() {
            let vals: Vec<u16> = self.w_val.split(|c| c == ',' || c == ' ').filter_map(|t| t.trim().parse::<u16>().ok()).collect();
            pending.push(Cmd::ReadWriteMulti { unit: self.w_unit, read_addr: self.w_addr, read_qty: self.w_qty, write_addr: self.w_addr, values: vals });
            self.modal = Modal::None;
        }
    }

    fn body_diag(&mut self, ui: &mut egui::Ui, snap: &Snap, pending: &mut Vec<Cmd>) {
        ui.horizontal(|ui| { ui.label("Unit"); ui.add(egui::DragValue::new(&mut self.w_unit).range(0..=255)); });
        egui::ComboBox::from_id_source("diagsub").selected_text(diag_sub_name(self.diag_sub)).show_ui(ui, |ui| {
            for sub in [0x0000u16, 0x0001, 0x0002, 0x0004, 0x000A, 0x000B, 0x000C, 0x000D, 0x000E, 0x000F, 0x0010, 0x0011, 0x0012] {
                ui.selectable_value(&mut self.diag_sub, sub, format!("{sub:#06X} {}", diag_sub_name(sub)));
            }
        });
        ui.horizontal(|ui| { ui.label("Data"); ui.add(egui::DragValue::new(&mut self.diag_data).range(0..=65535)); });
        if ui.button("Send (FC 08)").clicked() {
            pending.push(Cmd::Diagnostics { unit: self.w_unit, sub: self.diag_sub, data: self.diag_data });
        }
        if let Some((sub, v)) = snap.diag {
            ui.separator();
            ui.monospace(format!("{sub:#06X} {} = {v}", diag_sub_name(sub)));
        }
    }

    fn body_comm_event(&mut self, ui: &mut egui::Ui, snap: &Snap, pending: &mut Vec<Cmd>) {
        ui.horizontal(|ui| { ui.label("Unit"); ui.add(egui::DragValue::new(&mut self.w_unit).range(0..=255)); });
        if ui.button("Send (FC 0B)").clicked() { pending.push(Cmd::CommEvent { unit: self.w_unit }); }
        if let Some((s, c)) = snap.comm_event { ui.separator(); ui.monospace(format!("status={s}  event_count={c}")); }
    }

    fn body_report_id(&mut self, ui: &mut egui::Ui, snap: &Snap, pending: &mut Vec<Cmd>) {
        ui.horizontal(|ui| { ui.label("Unit"); ui.add(egui::DragValue::new(&mut self.w_unit).range(0..=255)); });
        if ui.button("Send (FC 11)").clicked() { pending.push(Cmd::ReportId { unit: self.w_unit }); }
        if !snap.server_id.is_empty() {
            ui.separator();
            ui.monospace(hexs(&snap.server_id));
        }
    }

    fn body_device_id(&mut self, ui: &mut egui::Ui, snap: &Snap, pending: &mut Vec<Cmd>) {
        ui.horizontal(|ui| { ui.label("Unit"); ui.add(egui::DragValue::new(&mut self.w_unit).range(0..=255)); });
        ui.horizontal(|ui| { ui.label("Read code"); ui.add(egui::DragValue::new(&mut self.devid_code).range(1..=4)); ui.label("Object"); ui.add(egui::DragValue::new(&mut self.devid_obj).range(0..=255)); });
        if ui.button("Send (FC 2B / 0E)").clicked() { pending.push(Cmd::DeviceId { unit: self.w_unit, code: self.devid_code, obj: self.devid_obj }); }
        if !snap.device_id.is_empty() {
            ui.separator();
            for (id, s) in &snap.device_id { ui.monospace(format!("{id:02X}: {s}")); }
        }
    }

    fn body_address_scan(&mut self, ui: &mut egui::Ui, snap: &Snap, pending: &mut Vec<Cmd>) {
        ui.horizontal(|ui| { ui.label("Unit"); ui.add(egui::DragValue::new(&mut self.scan_unit).range(0..=255)); });
        ui.horizontal(|ui| { ui.label("From"); ui.add(egui::DragValue::new(&mut self.scan_start).range(0..=65535)); ui.label("To"); ui.add(egui::DragValue::new(&mut self.scan_end).range(0..=65535)); });
        ui.horizontal(|ui| {
            ui.label("Function");
            egui::ComboBox::from_id_source("scanfc").selected_text(fc_label(self.scan_fc)).show_ui(ui, |ui| {
                for fc in [FC_READ_COILS, FC_READ_DISCRETE, FC_READ_HOLDING, FC_READ_INPUT] { ui.selectable_value(&mut self.scan_fc, fc, fc_label(fc)); }
            });
        });
        ui.horizontal(|ui| {
            if ui.button("Start").clicked() { pending.push(Cmd::ScanAddress { unit: self.scan_unit, fc: self.scan_fc, start: self.scan_start, end: self.scan_end }); }
            ui.label(if snap.scan.running { format!("scanning {}/{}…", snap.scan.cur, snap.scan.end) } else { format!("{} found, {} errors", snap.scan.found.len(), snap.scan.errors) });
        });
    }

    fn body_slave_scan(&mut self, ui: &mut egui::Ui, snap: &Snap, pending: &mut Vec<Cmd>) {
        ui.horizontal(|ui| { ui.label("From ID"); ui.add(egui::DragValue::new(&mut self.slave_start).range(1..=247)); ui.label("To ID"); ui.add(egui::DragValue::new(&mut self.slave_end).range(1..=247)); });
        ui.horizontal(|ui| { ui.label("Probe address"); ui.add(egui::DragValue::new(&mut self.slave_addr).range(0..=65535)); });
        if ui.button("Start").clicked() { pending.push(Cmd::ScanSlave { fc: FC_READ_HOLDING, addr: self.slave_addr, start: self.slave_start, end: self.slave_end }); }
        ui.label(if snap.scan.running { format!("scanning {}/{}…", snap.scan.cur, snap.scan.end) } else { format!("{} slaves found", snap.scan.found.len()) });
    }
}

// --------------------------- free helpers ----------------------------------

/// Dark, accent-themed visuals so the app looks like a real instrument panel.
fn setup_style(ctx: &egui::Context) {
    use egui::{FontId, TextStyle};
    let mut style = (*ctx.style()).clone();
    let accent = Color32::from_rgb(86, 156, 214);
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(21, 23, 28);
    v.window_fill = Color32::from_rgb(28, 31, 37);
    v.extreme_bg_color = Color32::from_rgb(15, 17, 21);
    v.faint_bg_color = Color32::from_rgb(33, 36, 43);
    v.selection.bg_fill = accent;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.window_rounding = egui::Rounding::same(8.0);
    v.menu_rounding = egui::Rounding::same(8.0);
    v.widgets.noninteractive.bg_fill = Color32::from_rgb(30, 33, 40);
    v.widgets.inactive.bg_fill = Color32::from_rgb(45, 49, 58);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(37, 40, 48);
    v.widgets.inactive.rounding = egui::Rounding::same(6.0);
    v.widgets.hovered.bg_fill = Color32::from_rgb(60, 66, 78);
    v.widgets.hovered.rounding = egui::Rounding::same(6.0);
    v.widgets.active.bg_fill = accent;
    v.widgets.active.rounding = egui::Rounding::same(6.0);
    style.visuals = v;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.scroll.bar_width = 10.0;
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(20.0)),
        (TextStyle::Body, FontId::proportional(14.5)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
        (TextStyle::Button, FontId::proportional(14.0)),
        (TextStyle::Small, FontId::proportional(12.0)),
    ]
    .into();
    ctx.set_style(style);
}

fn pick_open(desc: &str, ext: &str) -> Option<String> {
    rfd::FileDialog::new()
        .add_filter(desc, &[ext])
        .pick_file()
        .map(|p| p.to_string_lossy().to_string())
}

fn pick_save(desc: &str, ext: &str, default_name: &str) -> Option<String> {
    rfd::FileDialog::new()
        .add_filter(desc, &[ext])
        .set_file_name(default_name)
        .save_file()
        .map(|p| p.to_string_lossy().to_string())
}

fn pick_dir() -> Option<String> {
    rfd::FileDialog::new().pick_folder().map(|p| p.to_string_lossy().to_string())
}

fn draw_cell(ui: &mut egui::Ui, w: f32, h: f32, text: &str, bg: [u8; 3], fg: [u8; 3], selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, Color32::from_rgb(bg[0], bg[1], bg[2]));
    if selected { p.rect_stroke(rect, 3.0, Stroke::new(2.0, Color32::from_rgb(90, 160, 255))); }
    p.text(rect.center(), Align2::CENTER_CENTER, text, FontId::monospace(13.0), Color32::from_rgb(fg[0], fg[1], fg[2]));
    resp
}

fn draw_cell_static(ui: &mut egui::Ui, w: f32, h: f32, text: &str, bg: [u8; 3], fg: [u8; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, Color32::from_rgb(bg[0], bg[1], bg[2]));
    p.text(rect.center(), Align2::CENTER_CENTER, text, FontId::monospace(13.0), Color32::from_rgb(fg[0], fg[1], fg[2]));
}

fn swatches(ui: &mut egui::Ui, cur: &mut [u8; 3]) {
    ui.horizontal_wrapped(|ui| {
        for c in PALETTE32.iter() {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
            ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(c[0], c[1], c[2]));
            if *cur == *c { ui.painter().rect_stroke(rect, 2.0, Stroke::new(2.0, Color32::WHITE)); }
            if resp.clicked() { *cur = *c; }
        }
    });
}

fn color_row(ui: &mut egui::Ui, label: &str, c: &mut [u8; 3]) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut c[0]).range(0..=255));
        ui.add(egui::DragValue::new(&mut c[1]).range(0..=255));
        ui.add(egui::DragValue::new(&mut c[2]).range(0..=255));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 16.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(c[0], c[1], c[2]));
    });
}

fn draw_traffic(ui: &mut egui::Ui, log: &[crate::store::TrafficEntry]) {
    ui.strong("Communication traffic");
    ui.separator();
    egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
        for e in log.iter().rev().take(400).rev() {
            let col = match e.dir { '!' => Color32::from_rgb(230, 110, 110), '<' => Color32::from_rgb(120, 220, 140), _ => Color32::LIGHT_GRAY };
            ui.colored_label(col, format!("{} {} {}", e.t, e.dir, e.text));
        }
    });
}

fn draw_scan(ui: &mut egui::Ui, snap: &Snap) {
    ui.strong(if snap.scan.kind == 0 { "Address scan" } else { "Slave scan" });
    ui.label(format!("{} results, {} errors", snap.scan.found.len(), snap.scan.errors));
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (a, v) in &snap.scan.found {
            ui.monospace(format!("{a:6}  {v}"));
        }
    });
}

fn parse_hex(s: &str) -> Option<Vec<u8>> {
    let toks: Vec<&str> = s.split(|c: char| c == ' ' || c == ',' || c == '-').filter(|t| !t.is_empty()).collect();
    let mut out = Vec::new();
    for t in toks {
        out.push(u8::from_str_radix(t.trim_start_matches("0x"), 16).ok()?);
    }
    if out.is_empty() { None } else { Some(out) }
}

fn hexs(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

fn all_formats() -> Vec<ValueFormat> {
    let mut v = vec![ValueFormat::U16, ValueFormat::I16, ValueFormat::Hex16, ValueFormat::Bin16, ValueFormat::Ascii16, ValueFormat::U16Swapped];
    for o in [WordOrder::BigEndian, WordOrder::LittleEndian, WordOrder::BigEndianByteSwap, WordOrder::LittleEndianByteSwap] {
        v.push(ValueFormat::U32(o));
        v.push(ValueFormat::I32(o));
        v.push(ValueFormat::Hex32(o));
        v.push(ValueFormat::F32(o));
        v.push(ValueFormat::U64(o));
        v.push(ValueFormat::I64(o));
        v.push(ValueFormat::F64(o));
    }
    v
}
