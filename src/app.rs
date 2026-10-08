//! egui front-end: register grid, Conditional-Colours editor, SCADA canvas,
//! strip chart, communication traffic, connection/poll-group dialogs.

use crate::colors::{contrast, ColorMap, ColorMode, ColorOp, ColorRule, Palette, PALETTE32};
use crate::formats::{ValueFormat, WordOrder};
use crate::modbus::{fc_label, ConnConfig, Mode, FC_READ_COILS, FC_READ_DISCRETE, FC_READ_HOLDING, FC_READ_INPUT};
use crate::scada::{autolayout, Tile, TileKind};
use crate::store::{
    is_bit_fc_key, value_of, CellKey, Cmd, ConnState, PollGroup, Shared, SharedHandle,
};
use crate::workspace::Project;
use eframe::egui;
use egui::{Align2, Color32, FontId, Stroke};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Grid,
    Scada,
    Chart,
    Traffic,
}

struct Snap {
    words: HashMap<CellKey, u16>,
    bits: HashMap<CellKey, bool>,
    history: HashMap<CellKey, VecDeque<f64>>,
    stats: crate::store::Stats,
    traffic: Vec<Vec<crate::store::TrafficEntry>>,
    conn: ConnState,
}

pub struct MbApp {
    project: Project,
    shared: SharedHandle,
    cmd: Option<Sender<Cmd>>,
    worker: Option<std::thread::JoinHandle<()>>,

    view: View,
    selected: Option<CellKey>,
    write_buf: String,
    path_buf: String,
    status: String,
    active_group: usize,
    sel_tile: Option<usize>,

    show_conn: bool,
    show_colors: bool,
    show_scale: bool,
    show_names: bool,
    show_about: bool,
    new_name_key: u16,
    new_name_val: String,
}

impl MbApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let shared: SharedHandle = Arc::new(Mutex::new(Shared::default()));
        let project = Project::default();
        let (cmd, worker) = crate::store::spawn(project.conn.clone(), shared.clone());
        // Seed the worker with the default group.
        for g in &project.groups {
            let _ = cmd.send(Cmd::AddGroup(g.clone()));
        }
        Self {
            project,
            shared,
            cmd: Some(cmd),
            worker: Some(worker),
            view: View::Grid,
            selected: None,
            write_buf: String::new(),
            path_buf: "/home/lain/mbrs-workspace.mbw".into(),
            status: "ready".into(),
            active_group: 0,
            sel_tile: None,
            show_conn: false,
            show_colors: false,
            show_scale: false,
            show_names: false,
            show_about: false,
            new_name_key: 0,
            new_name_val: String::new(),
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
            traffic: Vec::new(),
            conn: s.conn.clone(),
        }
    }

    fn traffic(&self) -> Vec<crate::store::TrafficEntry> {
        self.shared.lock().unwrap().traffic.iter().cloned().collect()
    }

    fn active(&self) -> Option<&PollGroup> {
        self.project.groups.get(self.active_group)
    }

    /// Load a workspace file (CLI argument `mbrs <workspace.mbw>`).
    pub fn load_workspace_file(&mut self, path: &str) {
        match Project::load(path) {
            Ok(p) => {
                self.project = p;
                self.path_buf = path.to_string();
                self.active_group = 0;
                self.restart_worker();
                self.status = format!("loaded {path}");
            }
            Err(e) => self.status = format!("load failed: {e}"),
        }
    }
}

impl eframe::App for MbApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut pending: Vec<Cmd> = Vec::new();
        let snap = self.snapshot(self.view == View::Chart);
        let traffic = if self.view == View::Traffic { self.traffic() } else { Vec::new() };

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New workspace").clicked() {
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
                    ui.label("workspace path:");
                    ui.text_edit_singleline(&mut self.path_buf);
                    if ui.button("Save workspace").clicked() {
                        match self.project.save(&self.path_buf) {
                            Ok(_) => self.status = format!("saved {}", self.path_buf),
                            Err(e) => self.status = format!("save failed: {e}"),
                        }
                        ui.close_menu();
                    }
                    if ui.button("Open workspace").clicked() {
                        match Project::load(&self.path_buf) {
                            Ok(p) => {
                                self.project = p;
                                self.active_group = 0;
                                pending.push(Cmd::ClearGroups);
                                for g in &self.project.groups {
                                    pending.push(Cmd::AddGroup(g.clone()));
                                }
                                self.restart_worker();
                                self.status = format!("loaded {}", self.path_buf);
                            }
                            Err(e) => self.status = format!("load failed: {e}"),
                        }
                        ui.close_menu();
                    }
                    if ui.button("Export CSV…").clicked() {
                        match self.export_csv() {
                            Ok(p) => self.status = format!("csv -> {p}"),
                            Err(e) => self.status = format!("csv failed: {e}"),
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Connection", |ui| {
                    if ui.button("Setup…").clicked() {
                        self.show_conn = true;
                        ui.close_menu();
                    }
                    if ui.button("Reconnect").clicked() {
                        self.restart_worker();
                        ui.close_menu();
                    }
                });
                ui.menu_button("Display", |ui| {
                    ui.label("Display format (active group):");
                    let fmts = all_formats();
                    for f in fmts {
                        let active = self.active().map(|g| g.format == f).unwrap_or(false);
                        if ui.selectable_label(active, f.label()).clicked() {
                            if let Some(g) = self.project.groups.get_mut(self.active_group) {
                                g.format = f;
                                let gc = g.clone();
                                pending.push(Cmd::UpdateGroup(gc));
                            }
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui.button("Conditional colours…").clicked() {
                        self.show_colors = true;
                        ui.close_menu();
                    }
                    if ui.button("Scaling…").clicked() {
                        self.show_scale = true;
                        ui.close_menu();
                    }
                    if ui.button("Value names…").clicked() {
                        self.show_names = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.selectable_label(self.view == View::Grid, "Register grid").clicked() {
                        self.view = View::Grid;
                        ui.close_menu();
                    }
                    if ui.selectable_label(self.view == View::Scada, "SCADA dashboard").clicked() {
                        self.view = View::Scada;
                        ui.close_menu();
                    }
                    if ui.selectable_label(self.view == View::Chart, "Strip chart").clicked() {
                        self.view = View::Chart;
                        ui.close_menu();
                    }
                    if ui.selectable_label(self.view == View::Traffic, "Communication traffic").clicked() {
                        self.view = View::Traffic;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("About mbrs").clicked() {
                        self.show_about = true;
                        ui.close_menu();
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (txt, col) = match &snap.conn {
                        ConnState::Connected => ("● connected", Color32::from_rgb(80, 220, 120)),
                        ConnState::Connecting => ("● connecting", Color32::from_rgb(230, 200, 60)),
                        ConnState::Error(e) => {
                            ui.colored_label(Color32::from_rgb(230, 90, 90), format!("● {e}"));
                            ui.label("|");
                            ("", Color32::GRAY)
                        }
                        ConnState::Disconnected => ("● disconnected", Color32::from_rgb(180, 180, 180)),
                    };
                    if !txt.is_empty() {
                        ui.colored_label(col, txt);
                    }
                });
            });
        });

        // ---- side panel: connection + groups + stats -------------------------
        egui::SidePanel::left("side")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
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
                    if ui.button("Reconnect").clicked() {
                        self.restart_worker();
                    }
                });
                ui.separator();
                ui.label("Poll groups:");
                let mut remove: Option<usize> = None;
                egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                    for (i, g) in self.project.groups.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            let sel = self.active_group == i;
                            if ui.selectable_label(sel, format!("{} [{}]", g.name, g.id)).clicked() {
                                self.active_group = i;
                            }
                            if ui.checkbox(&mut g.enabled, "").changed() {
                                let gc = g.clone();
                                pending.push(Cmd::UpdateGroup(gc));
                            }
                            if ui.small_button("x").clicked() {
                                remove = Some(i);
                            }
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
                ui.label("Statistics");
                ui.label(format!("ok {}   err {}", snap.stats.ok, snap.stats.err));
                ui.label(format!("timeouts {}   last {:.1} ms", snap.stats.timeouts, snap.stats.last_ms));
                if !snap.stats.last_error.is_empty() {
                    ui.colored_label(Color32::from_rgb(230, 120, 120), &snap.stats.last_error);
                }
                ui.separator();
                ui.label(format!("Status: {}", self.status));
            });

        // ---- central ---------------------------------------------------------
        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Grid => self.draw_grid(ui, &snap, &mut pending),
            View::Scada => self.draw_scada(ui, &snap, &mut pending),
            View::Chart => self.draw_chart(ui, &snap),
            View::Traffic => draw_traffic(ui, &traffic),
        });

        // ---- dialogs ---------------------------------------------------------
        if self.show_conn {
            self.dialog_conn(ctx, &mut pending);
        }
        if self.show_colors {
            self.dialog_colors(ctx);
        }
        if self.show_scale {
            self.dialog_scale(ctx);
        }
        if self.show_names {
            self.dialog_names(ctx);
        }
        if self.show_about {
            egui::Window::new("About").open(&mut self.show_about).show(ctx, |ui| {
                ui.label("mbrs — Modbus Studio (Rust)");
                ui.label("A from-scratch Modbus master/monitor.");
                ui.label("Colour engine: N ordered rules + 32-level / smooth ramps.");
                ui.label("This is an independent reimplementation, not derived from Modbus Poll code.");
            });
        }

        for c in pending {
            self.send(c);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(120));
    }
}

impl MbApp {
    fn draw_grid(&mut self, ui: &mut egui::Ui, snap: &Snap, pending: &mut Vec<Cmd>) {
        let Some(g) = self.project.groups.get(self.active_group).cloned() else {
            ui.label("No poll group. Use ＋ group.");
            return;
        };
        ui.horizontal(|ui| {
            ui.strong(format!("{}", g.name));
            ui.separator();
            ui.label(fc_label(g.fc));
            ui.separator();
            ui.label(format!("start {}  count {}  scan {} ms", g.start, g.count, g.scan_ms));
            ui.separator();
            ui.label(g.format.label());
            ui.separator();
            if ui.button("Edit group…").clicked() {
                self.show_conn = false;
                self.show_colors = true;
            }
        });
        ui.separator();

        // write bar
        ui.horizontal(|ui| {
            if let Some(k) = self.selected {
                ui.label(format!("selected {} @ {}", fc_label(k.fc), k.addr));
                ui.label("value:");
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
                        self.status = "cannot parse value for current format".into();
                    }
                }
                if ui.button("Write 0").clicked() {
                    pending.push(Cmd::WriteReg { unit: k.unit, addr: k.addr, value: 0 });
                }
            } else {
                ui.label("click a cell to select it for writing");
            }
        });
        ui.separator();

        let n = g.format.regs_needed();
        let row_h = 22.0;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for i in 0..g.count {
                let addr = g.start.wrapping_add(i);
                let key = CellKey::new(g.unit, g.fc, addr);
                ui.horizontal(|ui| {
                    ui.monospace(format!("{:5}", addr));
                    if is_bit_fc_key(key) {
                        let on = snap.bits.get(&key).copied().unwrap_or(false);
                        let val = if on { 1.0 } else { 0.0 };
                        let (bg, fg) = g.color.color_for(val, if on { 1 } else { 0 });
                        let txt = if on { "1" } else { "0" };
                        let resp = draw_cell(ui, 90.0, row_h, txt, bg, fg, self.selected == Some(key));
                        if resp.clicked() {
                            self.selected = Some(key);
                            self.write_buf = if on { "1".into() } else { "0".into() };
                        }
                        ui.label(if on { "ON" } else { "off" });
                    } else {
                        let raw = snap.words.get(&key).copied();
                        // raw hex pill
                        let raws = raw.map(|v| format!("{v:04X}")).unwrap_or_else(|| "----".into());
                        draw_cell_static(ui, 70.0, row_h, &raws, [40, 44, 52], [180, 190, 200]);
                        // formatted value cell
                        let aligned = i % n as u16 == 0;
                        if aligned {
                            let regs: Vec<u16> = (0..n as u16)
                                .filter_map(|o| snap.words.get(&CellKey::new(g.unit, g.fc, addr.wrapping_add(o))).copied())
                                .collect();
                            if regs.len() == n {
                                let numeric = g.format.numeric(&regs);
                                let scaled = g.scale.apply(numeric);
                                let shown = if g.scale.enabled { scale_fmt(scaled, g.scale.decimals) } else { g.format.format(&regs) };
                                let (bg, fg) = g.color.color_for(scaled, regs[0] as u64);
                                let resp = draw_cell(ui, 190.0, row_h, &shown, bg, fg, self.selected == Some(key));
                                if resp.clicked() {
                                    self.selected = Some(key);
                                    self.write_buf = g.format.format(&regs);
                                }
                                ui.label(name_for(&self.project.names, regs[0]));
                            } else {
                                draw_cell_static(ui, 190.0, row_h, "…", [40, 44, 52], [120, 120, 120]);
                            }
                        } else {
                            draw_cell_static(ui, 190.0, row_h, "(cont)", [34, 36, 42], [90, 90, 100]);
                        }
                    }
                });
            }
        });
    }

    fn draw_scada(&mut self, ui: &mut egui::Ui, snap: &Snap, _pending: &mut Vec<Cmd>) {
        ui.horizontal(|ui| {
            ui.strong("SCADA dashboard");
            if ui.button("＋ tile from group").clicked() {
                if let Some(g) = self.project.groups.get(self.active_group) {
                    let id = self.project.tiles.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                    let mut t = Tile::new(id, g.start);
                    t.unit = g.unit;
                    t.fc = g.fc;
                    t.format = g.format;
                    t.label = g.name.clone();
                    t.lo = g.color.ramp_lo;
                    t.hi = g.color.ramp_hi;
                    t.color = g.color.clone();
                    self.project.tiles.push(t);
                    autolayout(&mut self.project.tiles, 4);
                }
            }
            if ui.button("Auto-layout").clicked() {
                autolayout(&mut self.project.tiles, 4);
            }
            if let Some(i) = self.sel_tile {
                if ui.button("Delete tile").clicked() {
                    self.project.tiles.remove(i);
                    self.sel_tile = None;
                }
            }
        });
        ui.separator();
        let size = ui.available_size();
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::click());
        let p = ui.painter();
        p.rect_filled(rect, 4.0, Color32::from_rgb(12, 12, 16));
        for (i, t) in self.project.tiles.iter().enumerate() {
            let tr = egui::Rect::from_min_size(
                rect.min + egui::vec2(t.x * rect.width(), t.y * rect.height()),
                egui::vec2(t.w * rect.width(), t.h * rect.height()),
            );
            let key = CellKey::new(t.unit, t.fc, t.addr);
            let raw = if is_bit_fc_key(key) {
                snap.bits.get(&key).copied().map(|b| if b { 1u64 } else { 0 }).unwrap_or(0)
            } else {
                snap.words.get(&key).copied().unwrap_or(0) as u64
            };
            let regs: Vec<u16> = (0..t.format.regs_needed() as u16)
                .filter_map(|o| snap.words.get(&CellKey::new(t.unit, t.fc, t.addr.wrapping_add(o))).copied())
                .collect();
            let numeric = if is_bit_fc_key(key) {
                raw as f64
            } else if regs.len() == t.format.regs_needed() {
                t.format.numeric(&regs)
            } else {
                f64::NAN
            };
            let (bg, fg) = t.color.color_for(numeric, raw);
            let bgc = Color32::from_rgb(bg[0], bg[1], bg[2]);
            let fgc = Color32::from_rgb(fg[0], fg[1], fg[2]);
            p.rect_filled(tr, 6.0, bgc);
            p.rect_stroke(tr, 6.0, Stroke::new(if self.sel_tile == Some(i) { 2.5 } else { 1.0 }, if self.sel_tile == Some(i) { Color32::from_rgb(90, 160, 255) } else { Color32::from_rgb(60, 60, 70) }));
            let title = if t.label.is_empty() { format!("REG {}", t.addr) } else { t.label.clone() };
            p.text(tr.left_top() + egui::vec2(8.0, 6.0), Align2::LEFT_TOP, &title, FontId::proportional(13.0), fgc);
            match t.kind {
                TileKind::Value => {
                    let txt = if is_bit_fc_key(key) {
                        if raw == 1 { "ON".to_string() } else { "OFF".to_string() }
                    } else if regs.len() == t.format.regs_needed() {
                        t.format.format(&regs)
                    } else {
                        "----".into()
                    };
                    p.text(tr.center() + egui::vec2(0.0, 6.0), Align2::CENTER_CENTER, txt, FontId::monospace(26.0), fgc);
                }
                TileKind::Lamp => {
                    let r = tr.height().min(tr.width()) * 0.22;
                    p.circle_filled(tr.center() + egui::vec2(0.0, 6.0), r, bgc);
                    p.circle_stroke(tr.center() + egui::vec2(0.0, 6.0), r, Stroke::new(2.0, fgc));
                    p.text(tr.center() + egui::vec2(0.0, r + 18.0), Align2::CENTER_CENTER, if raw == 1 { "ON" } else { "OFF" }, FontId::proportional(12.0), fgc);
                }
                TileKind::Bar | TileKind::Gauge => {
                    let inner = egui::Rect::from_min_max(tr.min + egui::vec2(10.0, tr.height() * 0.55), tr.max - egui::vec2(10.0, 10.0));
                    p.rect_filled(inner, 3.0, Color32::from_rgb(20, 20, 26));
                    let span = (t.hi - t.lo).abs();
                    let frac = if span < 1e-9 { 0.0 } else { ((numeric - t.lo) / span).clamp(0.0, 1.0) } as f32;
                    let fill = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width() * frac, inner.height()));
                    p.rect_filled(fill, 3.0, bgc);
                    p.rect_stroke(inner, 3.0, Stroke::new(1.0, Color32::from_rgb(70, 70, 80)));
                    let txt = if regs.len() == t.format.regs_needed() { t.format.format(&regs) } else { "----".into() };
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
        let Some(key) = key else {
            ui.label("no series");
            return;
        };
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
        let mn = hist.iter().cloned().fold(f64::INFINITY, f64::min);
        let mx = hist.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let span = if (mx - mn).abs() < 1e-9 { 1.0 } else { mx - mn };
        let pts: Vec<egui::Pos2> = hist
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let x = rect.left() + rect.width() * (i as f32 / (hist.len().max(2) - 1) as f32);
                let y = rect.bottom() - rect.height() * (((v - mn) / span) as f32);
                egui::pos2(x, y)
            })
            .collect();
        for w in pts.windows(2) {
            p.line_segment([w[0], w[1]], Stroke::new(1.5, Color32::from_rgb(90, 200, 255)));
        }
        p.text(
            rect.left_top() + egui::vec2(8.0, 6.0),
            Align2::LEFT_TOP,
            format!("{label}  min={mn:.3} max={mx:.3} last={:.3} n={}", hist.back().copied().unwrap_or(0.0), hist.len()),
            FontId::monospace(12.0),
            Color32::LIGHT_GRAY,
        );
    }

    fn export_csv(&self) -> anyhow::Result<String> {
        let s = self.shared.lock().unwrap();
        let mut out = String::from("unit,fc,addr,value\n");
        let mut keys: Vec<&CellKey> = s.words.keys().collect();
        keys.sort_by_key(|k| (k.unit, k.fc, k.addr));
        for k in keys {
            out.push_str(&format!("{},{},0x{:04X},{}\n", k.unit, fc_label(k.fc), k.addr, s.words[k]));
        }
        let path = format!("{}.csv", self.path_buf.trim_end_matches(".mbw"));
        std::fs::write(&path, out)?;
        Ok(path)
    }

    fn dialog_conn(&mut self, ctx: &egui::Context, _pending: &mut Vec<Cmd>) {
        let mut open = self.show_conn;
        egui::Window::new("Connection setup").open(&mut open).show(ctx, |ui| {
            let c = &mut self.project.conn;
            ui.horizontal(|ui| {
                ui.label("Mode:");
                egui::ComboBox::from_id_source("mode")
                    .selected_text(c.mode.label())
                    .show_ui(ui, |ui| {
                        for m in [Mode::Tcp, Mode::Rtu, Mode::Ascii, Mode::RtuOverTcp, Mode::AsciiOverTcp, Mode::Udp] {
                            ui.selectable_value(&mut c.mode, m, m.label());
                        }
                    });
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
            ui.separator();
            if ui.button("Apply & reconnect").clicked() {
                let cfg = self.project.conn.clone();
                self.restart_worker();
                let _ = cfg;
                self.status = "reconnecting".into();
            }
        });
        self.show_conn = open;
    }

    fn dialog_colors(&mut self, ctx: &egui::Context) {
        let mut open = self.show_colors;
        let mut pending: Vec<Cmd> = Vec::new();
        let Some(g) = self.project.groups.get(self.active_group).cloned() else {
            self.show_colors = false;
            return;
        };
        let mut map = g.color.clone();
        egui::Window::new(format!("Conditional colours — {}", g.name))
            .open(&mut open)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Mode:");
                    for m in ColorMode::ALL {
                        ui.selectable_value(&mut map.mode, *m, m.label());
                    }
                });
                if map.mode == ColorMode::Rules {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Normal");
                        color_row(ui, "bg", &mut map.normal_bg);
                        color_row(ui, "fg", &mut map.normal_fg);
                        ui.checkbox(&mut map.auto_fg, "auto fg");
                    });
                    if map.auto_fg {
                        map.normal_fg = contrast(map.normal_bg);
                    }
                    ui.separator();
                    let mut del: Option<usize> = None;
                    for (i, r) in map.rules.iter_mut().enumerate() {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut r.enabled, "");
                            ui.label(format!("Rule {}", i + 1));
                            egui::ComboBox::from_id_source(format!("op{i}"))
                                .selected_text(r.op.label())
                                .show_ui(ui, |ui| {
                                    for op in ColorOp::ALL {
                                        ui.selectable_value(&mut r.op, *op, op.label());
                                    }
                                });
                            ui.add(egui::DragValue::new(&mut r.value).speed(1.0));
                            if r.op == ColorOp::Range {
                                ui.label("..");
                                ui.add(egui::DragValue::new(&mut r.value2).speed(1.0));
                            }
                            ui.text_edit_singleline(&mut r.label);
                            if ui.small_button("del").clicked() {
                                del = Some(i);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("bg");
                            swatches(ui, &mut r.bg);
                            ui.label("fg");
                            swatches(ui, &mut r.fg);
                            ui.checkbox(&mut r.auto_fg, "auto");
                            if r.auto_fg {
                                r.fg = contrast(r.bg);
                            }
                        });
                    }
                    if let Some(i) = del {
                        map.rules.remove(i);
                    }
                    ui.separator();
                    if ui.button("＋ add rule").clicked() {
                        let bg = PALETTE32[map.rules.len() % 32];
                        map.rules.push(ColorRule::new(ColorOp::Gt, 0, bg));
                    }
                    ui.label("Rules are evaluated top-down; first match wins. Use BitAll/BitAny for bitmasks.");
                } else {
                    ui.separator();
                    egui::Grid::new("ramp").show(ui, |ui| {
                        ui.label("Range low"); ui.add(egui::DragValue::new(&mut map.ramp_lo).speed(1.0)); ui.end_row();
                        ui.label("Range high"); ui.add(egui::DragValue::new(&mut map.ramp_hi).speed(1.0)); ui.end_row();
                        ui.label("Levels (2..32)"); ui.add(egui::Slider::new(&mut map.levels, 2..=32)); ui.end_row();
                        ui.label("Palette");
                        egui::ComboBox::from_id_source("pal")
                            .selected_text(map.palette.label())
                            .show_ui(ui, |ui| {
                                for p in Palette::ALL {
                                    ui.selectable_value(&mut map.palette, *p, p.label());
                                }
                            });
                        ui.end_row();
                    });
                    ui.separator();
                    ui.label("Preview:");
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
                    ui.label("Discrete mode maps the value range to exactly `Levels` distinct colours — up to 32.");
                }
                ui.separator();
                if ui.button("Apply to active group").clicked() {
                    if let Some(slot) = self.project.groups.get_mut(self.active_group) {
                        slot.color = map.clone();
                        let gc = slot.clone();
                        pending.push(Cmd::UpdateGroup(gc));
                    }
                    self.status = "colours applied".into();
                }
            });
        self.show_colors = open;
        for c in pending {
            self.send(c);
        }
    }

    fn dialog_scale(&mut self, ctx: &egui::Context) {
        let mut open = self.show_scale;
        let g = self.project.groups.get(self.active_group).cloned();
        let Some(mut g) = g else {
            self.show_scale = false;
            return;
        };
        let mut apply = false;
        egui::Window::new("Scaling").open(&mut open).show(ctx, |ui| {
            ui.checkbox(&mut g.scale.enabled, "Enable scaling");
            egui::Grid::new("scale").show(ui, |ui| {
                ui.label("X1"); ui.add(egui::DragValue::new(&mut g.scale.x1)); ui.end_row();
                ui.label("Y1"); ui.add(egui::DragValue::new(&mut g.scale.y1)); ui.end_row();
                ui.label("X2"); ui.add(egui::DragValue::new(&mut g.scale.x2)); ui.end_row();
                ui.label("Y2"); ui.add(egui::DragValue::new(&mut g.scale.y2)); ui.end_row();
                ui.label("Decimals"); ui.add(egui::DragValue::new(&mut g.scale.decimals).range(0..=6)); ui.end_row();
            });
            ui.label("Y = m·(X − X1) + Y1,  m = (Y2 − Y1)/(X2 − X1)");
            if ui.button("Apply").clicked() {
                apply = true;
            }
        });
        if apply {
            if let Some(slot) = self.project.groups.get_mut(self.active_group) {
                slot.scale = g.scale;
                let gc = slot.clone();
                self.send(Cmd::UpdateGroup(gc));
            }
        }
        self.show_scale = open;
    }

    fn dialog_names(&mut self, ctx: &egui::Context) {
        let mut open = self.show_names;
        egui::Window::new("Value names").open(&mut open).show(ctx, |ui| {
            ui.label("Map register value -> descriptive text (0=Ready, 1=Running …).");
            ui.horizontal(|ui| {
                ui.label("value");
                ui.add(egui::DragValue::new(&mut self.new_name_key).range(0..=65535));
                ui.label("text");
                ui.text_edit_singleline(&mut self.new_name_val);
                if ui.button("Add").clicked() {
                    self.project.names.retain(|(k, _)| *k != self.new_name_key);
                    self.project.names.push((self.new_name_key, self.new_name_val.clone()));
                    self.project.names.sort_by_key(|(k, _)| *k);
                    self.new_name_val.clear();
                }
            });
            ui.separator();
            let mut del: Option<usize> = None;
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for (i, (k, v)) in self.project.names.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{k:>5}"));
                        ui.text_edit_singleline(v);
                        if ui.small_button("x").clicked() {
                            del = Some(i);
                        }
                    });
                }
            });
            if let Some(i) = del {
                self.project.names.remove(i);
            }
        });
        self.show_names = open;
    }
}

// ---------------------------------------------------------------------------
// free helpers

fn draw_cell(ui: &mut egui::Ui, w: f32, h: f32, text: &str, bg: [u8; 3], fg: [u8; 3], selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, Color32::from_rgb(bg[0], bg[1], bg[2]));
    if selected {
        p.rect_stroke(rect, 3.0, Stroke::new(2.0, Color32::from_rgb(90, 160, 255)));
    }
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
            if *cur == *c {
                ui.painter().rect_stroke(rect, 2.0, Stroke::new(2.0, Color32::WHITE));
            }
            if resp.clicked() {
                *cur = *c;
            }
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
            let col = match e.dir {
                '!' => Color32::from_rgb(230, 110, 110),
                '<' => Color32::from_rgb(120, 220, 140),
                _ => Color32::LIGHT_GRAY,
            };
            ui.colored_label(col, format!("{} {} {}", e.t, e.dir, e.text));
        }
    });
}

fn name_for(names: &[(u16, String)], raw: u16) -> String {
    names.iter().find(|(k, _)| *k == raw).map(|(_, v)| v.clone()).unwrap_or_default()
}

fn scale_fmt(v: f64, dec: u8) -> String {
    format!("{:.*}", dec as usize, v)
}

fn all_formats() -> Vec<ValueFormat> {
    let mut v = vec![
        ValueFormat::U16,
        ValueFormat::I16,
        ValueFormat::Hex16,
        ValueFormat::Bin16,
        ValueFormat::Ascii16,
        ValueFormat::U16Swapped,
    ];
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
