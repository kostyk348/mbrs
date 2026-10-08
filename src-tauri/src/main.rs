//! mbrs product back-end (Tauri v2). Thin command layer over the mbrs core:
//! one polling worker per connection, shared snapshot, group CRUD, writes,
//! scan, diagnostics and workspace I/O.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mbrs::modbus::ConnConfig;
use mbrs::store::{CellKey, Cmd, PollGroup, Shared, SharedHandle};
use mbrs::workspace::Project;
use serde_json::{json, Value};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

struct AppState {
    shared: SharedHandle,
    cmd: Mutex<Option<Sender<Cmd>>>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    groups: Mutex<Vec<PollGroup>>,
    conn: Mutex<ConnConfig>,
}

fn key_str(k: &CellKey) -> String {
    format!("{}:{}:{}", k.unit, k.fc, k.addr)
}

fn restart(state: &AppState) {
    if let Some(tx) = state.cmd.lock().unwrap().take() {
        let _ = tx.send(Cmd::Stop);
    }
    if let Some(h) = state.worker.lock().unwrap().take() {
        let _ = h.join();
    }
    let conn = state.conn.lock().unwrap().clone();
    let (tx, h) = mbrs::store::spawn(conn, state.shared.clone());
    for g in state.groups.lock().unwrap().iter() {
        let _ = tx.send(Cmd::AddGroup(g.clone()));
    }
    *state.cmd.lock().unwrap() = Some(tx);
    *state.worker.lock().unwrap() = Some(h);
}

fn send(state: &AppState, c: Cmd) {
    if let Some(tx) = state.cmd.lock().unwrap().as_ref() {
        let _ = tx.send(c);
    }
}

// ---- queries ---------------------------------------------------------------

#[tauri::command]
fn snapshot(state: State<AppState>) -> Value {
    let s = state.shared.lock().unwrap();
    let words: serde_json::Map<String, Value> =
        s.words.iter().map(|(k, v)| (key_str(k), json!(v))).collect();
    let bits: serde_json::Map<String, Value> =
        s.bits.iter().map(|(k, v)| (key_str(k), json!(v))).collect();
    let groups = state.groups.lock().unwrap().clone();
    let conn = state.conn.lock().unwrap().clone();
    json!({
        "conn": conn,
        "state": s.conn,
        "stats": s.stats,
        "scan": s.scan,
        "words": words,
        "bits": bits,
        "groups": groups,
        "last_response": s.last_response,
        "comm_event": s.comm_event,
        "device_id": s.device_id,
        "server_id": s.server_id,
        "diag": s.diag,
        "traffic": s.traffic.iter().collect::<Vec<_>>(),
    })
}

// ---- connection ------------------------------------------------------------

#[tauri::command]
fn set_conn(state: State<AppState>, conn: ConnConfig) {
    *state.conn.lock().unwrap() = conn;
    restart(&state);
}

#[tauri::command]
fn set_unit(state: State<AppState>, unit: u8) {
    send(&state, Cmd::SetUnit(unit));
}

// ---- groups ----------------------------------------------------------------

#[tauri::command]
fn add_group(state: State<AppState>, mut g: PollGroup) -> Value {
    let id = {
        let groups = state.groups.lock().unwrap();
        if g.id == 0 || groups.iter().any(|x| x.id == g.id) {
            groups.iter().map(|x| x.id).max().unwrap_or(0) + 1
        } else {
            g.id
        }
    };
    g.id = id;
    send(&state, Cmd::AddGroup(g.clone()));
    state.groups.lock().unwrap().push(g.clone());
    json!(g)
}

#[tauri::command]
fn update_group(state: State<AppState>, g: PollGroup) {
    {
        let mut groups = state.groups.lock().unwrap();
        if let Some(slot) = groups.iter_mut().find(|x| x.id == g.id) {
            *slot = g.clone();
        }
    }
    send(&state, Cmd::UpdateGroup(g));
}

#[tauri::command]
fn remove_group(state: State<AppState>, id: u32) {
    state.groups.lock().unwrap().retain(|g| g.id != id);
    send(&state, Cmd::RemoveGroup(id));
}

// ---- writes / functions ----------------------------------------------------

#[tauri::command]
fn write_reg(state: State<AppState>, unit: u8, addr: u16, value: u16) {
    send(&state, Cmd::WriteReg { unit, addr, value });
}

#[tauri::command]
fn write_regs(state: State<AppState>, unit: u8, addr: u16, values: Vec<u16>) {
    send(&state, Cmd::WriteRegs { unit, addr, values });
}

#[tauri::command]
fn write_coil(state: State<AppState>, unit: u8, addr: u16, on: bool) {
    send(&state, Cmd::WriteCoil { unit, addr, on });
}

#[tauri::command]
fn write_coils(state: State<AppState>, unit: u8, addr: u16, values: Vec<bool>) {
    send(&state, Cmd::WriteCoils { unit, addr, values });
}

#[tauri::command]
fn mask_write(state: State<AppState>, unit: u8, addr: u16, and_mask: u16, or_mask: u16) {
    send(&state, Cmd::MaskWrite { unit, addr, and_mask, or_mask });
}

#[tauri::command]
fn read_write_multi(
    state: State<AppState>,
    unit: u8,
    read_addr: u16,
    read_qty: u16,
    write_addr: u16,
    values: Vec<u16>,
) {
    send(&state, Cmd::ReadWriteMulti { unit, read_addr, read_qty, write_addr, values });
}

#[tauri::command]
fn diag(state: State<AppState>, unit: u8, sub: u16, data: u16) {
    send(&state, Cmd::Diagnostics { unit, sub, data });
}

#[tauri::command]
fn comm_event(state: State<AppState>, unit: u8) {
    send(&state, Cmd::CommEvent { unit });
}

#[tauri::command]
fn report_id(state: State<AppState>, unit: u8) {
    send(&state, Cmd::ReportId { unit });
}

#[tauri::command]
fn device_id(state: State<AppState>, unit: u8, code: u8, obj: u8) {
    send(&state, Cmd::DeviceId { unit, code, obj });
}

#[tauri::command]
fn raw(state: State<AppState>, unit: u8, pdu: Vec<u8>) {
    send(&state, Cmd::Raw { unit, pdu });
}

#[tauri::command]
fn scan_address(state: State<AppState>, unit: u8, fc: u8, start: u16, end: u16) {
    send(&state, Cmd::ScanAddress { unit, fc, start, end });
}

#[tauri::command]
fn scan_slave(state: State<AppState>, fc: u8, addr: u16, start: u8, end: u8) {
    send(&state, Cmd::ScanSlave { fc, addr, start, end });
}

// ---- workspace -------------------------------------------------------------

#[tauri::command]
fn save_workspace(state: State<AppState>, path: String) -> Result<Value, String> {
    let p = Project {
        version: mbrs::workspace::FORMAT_VERSION,
        conn: state.conn.lock().unwrap().clone(),
        groups: state.groups.lock().unwrap().clone(),
        tiles: vec![],
        names: Default::default(),
        log: Default::default(),
    };
    p.save(&path).map_err(|e| e.to_string())?;
    Ok(json!({ "path": path }))
}

#[tauri::command]
fn load_workspace(state: State<AppState>, path: String) -> Result<Value, String> {
    let p = Project::load(&path).map_err(|e| e.to_string())?;
    *state.conn.lock().unwrap() = p.conn.clone();
    *state.groups.lock().unwrap() = p.groups.clone();
    restart(&state);
    Ok(json!({ "groups": p.groups, "conn": p.conn }))
}

// ---- native file dialogs ---------------------------------------------------

#[tauri::command]
fn pick_file(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog().file().blocking_pick_file().map(|p| p.to_string())
}

#[tauri::command]
fn pick_save(app: tauri::AppHandle, name: String) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .set_file_name(name)
        .blocking_save_file()
        .map(|p| p.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            snapshot,
            set_conn,
            set_unit,
            add_group,
            update_group,
            remove_group,
            write_reg,
            write_regs,
            write_coil,
            write_coils,
            mask_write,
            read_write_multi,
            diag,
            comm_event,
            report_id,
            device_id,
            raw,
            scan_address,
            scan_slave,
            save_workspace,
            load_workspace,
            pick_file,
            pick_save
        ])
        .setup(|app| {
            let shared: SharedHandle = Arc::new(Mutex::new(Shared::default()));
            let conn = ConnConfig::default();
            let (tx, h) = mbrs::store::spawn(conn.clone(), shared.clone());
            app.manage(AppState {
                shared,
                cmd: Mutex::new(Some(tx)),
                worker: Mutex::new(Some(h)),
                groups: Mutex::new(Vec::new()),
                conn: Mutex::new(conn),
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running mbrs");
}
