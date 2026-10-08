/* mbrs product front-end — talks to the Rust core via Tauri commands. */
const invoke = window.__TAURI__.core.invoke;

const FC = { 1: "01 Read Coils", 2: "02 Read Discrete", 3: "03 Read Holding", 4: "04 Read Input" };
const PALETTES = ["Traffic", "Rainbow", "Viridis", "Heat", "Cool", "Greys"];
const OPS = ["NotUsed", "Eq", "Ne", "Gt", "Ge", "Lt", "Le", "BitAll", "BitAny", "Range", "InSet"];

const state = {
  snap: null,
  view: "grid",
  groupId: null,
  selected: null,
  writeBuf: "",
  plcAddr: false,
  history: {},
  status: "ready",
};

/* ---------- colour engine (mirror of mbrs::colors) ---------- */
const lerp = (a, b, t) => a.map((x, i) => Math.round(x + (b[i] - x) * t));
function ramp(stops, t) {
  t = Math.max(0, Math.min(1, t));
  const seg = t * (stops.length - 1);
  const i = Math.min(Math.floor(seg), stops.length - 2);
  return lerp(stops[i], stops[i + 1], seg - i);
}
function palette(p, t) {
  switch (p) {
    case "Traffic": return ramp([[0,160,0],[230,200,0],[200,20,20]], t);
    case "Rainbow": return ramp([[40,60,200],[0,190,220],[40,180,40],[230,220,0],[210,30,30]], t);
    case "Viridis": return ramp([[68,1,84],[59,82,139],[33,145,140],[94,201,98],[253,231,37]], t);
    case "Heat": return ramp([[0,0,0],[150,0,0],[255,90,0],[255,230,120]], t);
    case "Cool": return ramp([[0,200,210],[0,80,200],[180,40,200]], t);
    default: return ramp([[30,30,30],[230,230,230]], t);
  }
}
const rgb = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;
function contrast(bg) {
  const lum = 0.2126*bg[0] + 0.7152*bg[1] + 0.0722*bg[2];
  return lum > 140 ? [15,15,15] : [235,235,235];
}
function match(op, rv, v, raw) {
  const vi = Math.round(v);
  switch (op) {
    case "Eq": return vi === rv;
    case "Ne": return vi !== rv;
    case "Gt": return v > rv;
    case "Ge": return v >= rv;
    case "Lt": return v < rv;
    case "Le": return v <= rv;
    case "BitAll": return rv !== 0 && (raw & rv) === rv;
    case "BitAny": return (raw & rv) !== 0;
    case "Range": return v >= rv;
    default: return false;
  }
}
function colorFor(cm, v, raw) {
  if (cm.mode === "Discrete" || cm.mode === "Smooth") {
    const span = cm.ramp_hi - cm.ramp_lo;
    let t = Math.abs(span) < 1e-9 ? 0 : (v - cm.ramp_lo) / span;
    t = Math.max(0, Math.min(1, t));
    let bg;
    if (cm.mode === "Discrete") {
      const lev = Math.max(2, Math.min(32, cm.levels));
      bg = palette(cm.palette, Math.round(t * (lev - 1)) / (lev - 1));
    } else bg = palette(cm.palette, t);
    return [bg, contrast(bg)];
  }
  for (const r of cm.rules || []) {
    if (r.enabled && r.op !== "NotUsed" && match(r.op, r.value, v, raw))
      return [r.bg, r.auto_fg ? contrast(r.bg) : r.fg];
  }
  return [cm.normal_bg, cm.normal_fg];
}

/* ---------- helpers ---------- */
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;" }[c]));
const key = (unit, fc, addr) => `${unit}:${fc}:${addr}`;
function fmtLabel(f) {
  if (typeof f === "string") return { U16:"16-bit Unsigned", I16:"16-bit Signed", Hex16:"16-bit Hex", Bin16:"16-bit Binary", Ascii16:"16-bit ASCII", U16Swapped:"16-bit Byte-swap" }[f] || f;
  const [k, o] = Object.entries(f)[0];
  const nice = { U32:"UInt32", I32:"Int32", Hex32:"Hex32", F32:"Float", U64:"UInt64", I64:"Int64", F64:"Double" }[k] || k;
  return `${nice} [${o}]`;
}
function fmtValue(f, regs) {
  const f32 = (v) => Number(v).toFixed(4).replace(/\.?0+$/, "");
  const asU32 = (o) => {
    const words = regs.map((w) => [(w >> 8) & 255, w & 255]);
    if (o === "LittleEndian" || o === "LittleEndianByteSwap") words.reverse();
    if (o === "BigEndianByteSwap" || o === "LittleEndianByteSwap") words.forEach((w) => w.reverse());
    const b = words.flat(); return (b[0]<<24>>>0) + (b[1]<<16) + (b[2]<<8) + b[3];
  };
  if (typeof f === "string") {
    const v = regs[0] ?? 0;
    switch (f) {
      case "U16": return String(v);
      case "I16": return String((v << 16) >> 16);
      case "Hex16": return v.toString(16).toUpperCase().padStart(4, "0");
      case "Bin16": return v.toString(2).padStart(16, "0");
      case "Ascii16": return String.fromCharCode((v >> 8) & 255, v & 255).replace(/[^\x20-\x7e]/g, ".");
      case "U16Swapped": return String((v >> 8) | ((v & 255) << 8));
    }
  }
  const [k, o] = Object.entries(f)[0];
  if (k === "U32") return String(asU32(o));
  if (k === "Hex32") return asU32(o).toString(16).toUpperCase().padStart(8, "0");
  if (k === "I32") return String(asU32(o) | 0);
  if (k === "F32") { const b = new Uint8Array([asU32(o) >> 24 & 255, asU32(o) >> 16 & 255, asU32(o) >> 8 & 255, asU32(o) & 255]); return f32(new DataView(b.buffer).getFloat32(0)); }
  return String(regs[0] ?? 0);
}
const regsNeeded = (f) => (typeof f === "string" ? 1 : (["U32","I32","Hex32","F32"].includes(Object.keys(f)[0]) ? 2 : 4));

/* ---------- menus / toolbar ---------- */
function buildMenus() {
  const M = [
    ["File", [
      ["New workspace", () => { state.snap = null; location.reload(); }],
      ["Open workspace…", async () => { const p = await invoke("pick_file"); if (p) await invoke("load_workspace", { path: p }); }],
      ["Save workspace…", async () => { const p = await invoke("pick_save", { name: "workspace.mbw" }); if (p) await invoke("save_workspace", { path: p }); }],
      ["---", null],
      ["Export CSV…", async () => { const p = await invoke("pick_save", { name: "mbrs-export.csv" }); if (p) exportCsv(p); }],
    ]],
    ["Connection", [
      ["Connect…\tF3", () => dlgConn()],
      ["Reconnect", () => invoke("set_conn", { conn: state.snap.conn })],
      ["---", null],
      ["Enable all", () => toggleAll(true)],
      ["Disable all", () => toggleAll(false)],
    ]],
    ["Functions", [
      ["05 Write Single Coil…", () => dlgFn("05")],
      ["06 Write Single Register…", () => dlgFn("06")],
      ["15 Write Multiple Coils…", () => dlgFn("15")],
      ["16 Write Multiple Registers…", () => dlgFn("16")],
      ["22 Mask Write Register…", () => dlgFn("22")],
      ["23 Read/Write Multiple…", () => dlgFn("23")],
      ["08 Diagnostics…", () => dlgFn("08")],
      ["11 Get Comm Event Counter…", () => dlgFn("0B")],
      ["17 Report Server ID…", () => dlgFn("11")],
      ["43/14 Read Device Identification…", () => dlgFn("2B")],
    ]],
    ["Setup", [
      ["Read/Write Definition…\tF8", () => dlgGroup()],
      ["Value Names…", () => dlgNames()],
      ["Log Setup…", () => dlgLog()],
      ["---", null],
      ["Address Scan…", () => { state.view = "scan"; dlgScan("addr"); }],
      ["Slave Scan…", () => { state.view = "scan"; dlgScan("slave"); }],
    ]],
    ["Display", [
      ["Signed", () => setFormat("I16")],
      ["Unsigned", () => setFormat("U16")],
      ["Hex", () => setFormat("Hex16")],
      ["Binary", () => setFormat("Bin16")],
      ["---", null],
      ["Conditional Colors…\tAlt+Shift+C", () => dlgColors()],
      ["Scaling…", () => dlgGroup()],
    ]],
    ["View", [
      ["Register grid", () => { state.view = "grid"; }],
      ["Strip chart", () => { state.view = "chart"; }],
      ["Communication traffic", () => { state.view = "traffic"; }],
      ["Scan result", () => { state.view = "scan"; }],
      ["Test center", () => { state.view = "test"; }],
    ]],
    ["Help", [
      ["About mbrs", () => dlgAbout()],
    ]],
  ];
  const bar = document.getElementById("menubar");
  bar.innerHTML = "";
  for (const [name, items] of M) {
    const m = document.createElement("div");
    m.className = "menu";
    const b = document.createElement("button");
    b.textContent = name;
    b.onclick = (e) => { e.stopPropagation(); document.querySelectorAll(".menu").forEach((x) => x.classList.remove("open")); m.classList.toggle("open"); };
    const box = document.createElement("div");
    box.className = "items";
    for (const [label, fn] of items) {
      if (label === "---") { box.appendChild(document.createElement("hr")); continue; }
      const it = document.createElement("button");
      it.textContent = label;
      it.onclick = (e) => { e.stopPropagation(); m.classList.remove("open"); fn && fn(); };
      box.appendChild(it);
    }
    m.appendChild(b); m.appendChild(box); bar.appendChild(m);
  }
  document.addEventListener("click", () => document.querySelectorAll(".menu").forEach((x) => x.classList.remove("open")));
}

async function toggleAll(on) {
  for (const g of state.snap.groups) { g.enabled = on; await invoke("update_group", { g }); }
}

function buildToolbar() {
  const tb = document.getElementById("toolbar");
  tb.innerHTML = `<span class="brand">mbrs</span>`;
  for (const [id, label] of [["grid","Grid"],["chart","Chart"],["traffic","Traffic"],["scan","Scan"],["test","Test"]]) {
    const b = document.createElement("button");
    b.className = "tab" + (state.view === id ? " active" : "");
    b.textContent = label;
    b.onclick = () => { state.view = id; buildToolbar(); };
    tb.appendChild(b);
  }
  const sep = document.createElement("span"); sep.className = "sep"; tb.appendChild(sep);
  for (const [label, fn] of [["Connect", () => dlgConn()], ["Definition", () => dlgGroup()], ["Colors", () => dlgColors()], ["Names", () => dlgNames()], ["Log", () => dlgLog()]]) {
    const b = document.createElement("button"); b.textContent = label; b.onclick = fn; tb.appendChild(b);
  }
}

/* ---------- side panel ---------- */
function buildSide() {
  const s = state.snap; if (!s) return;
  const side = document.getElementById("side");
  const c = s.conn;
  const connColor = s.state === "Connected" ? "var(--ok)" : (s.state === "Error" ? "var(--err)" : "var(--muted)");
  side.innerHTML = `
    <div class="block">
      <div class="title">Connection</div>
      <div><span class="dot" style="background:${connColor}"></span>${esc(s.state?.Error ? "Error: " + s.state.Error : s.state)}</div>
      <div class="kv"><span>${esc(c.mode)}</span><span>${esc(c.host)}:${c.port}</span></div>
      <div class="kv"><span>unit ${c.unit}</span><span>timeout ${c.timeout_ms} ms</span></div>
      <div class="actions" style="display:flex;gap:6px;margin-top:8px">
        <button onclick="dlgConn()">Setup…</button>
        <button onclick="invoke('set_conn',{conn:state.snap.conn})">Reconnect</button>
      </div>
    </div>
    <div class="block">
      <div class="title">Poll groups <button onclick="dlgGroup(true)">＋</button></div>
      <div class="grouplist">
        ${s.groups.map((g) => `
          <div class="group ${g.id === state.groupId ? "active" : ""}" onclick="selectGroup(${g.id})">
            <input type="checkbox" ${g.enabled ? "checked" : ""} onclick="event.stopPropagation();toggleGroup(${g.id},this.checked)"/>
            <span class="nm">${esc(g.name)} [${g.id}]</span>
            <button onclick="event.stopPropagation();removeGroup(${g.id})">✕</button>
          </div>`).join("")}
      </div>
    </div>
    <div class="block">
      <div class="title">Statistics</div>
      <div class="kv"><span>ok ${s.stats.ok}</span><span>err ${s.stats.err}</span></div>
      <div class="kv"><span>timeouts ${s.stats.timeouts}</span><span>${s.stats.last_ms.toFixed(1)} ms</span></div>
      ${s.stats.last_error ? `<div class="kv" style="color:var(--err)">${esc(s.stats.last_error)}</div>` : ""}
    </div>
    <div class="block"><div class="kv"><span>Status</span><span>${esc(state.status)}</span></div></div>`;
}

function group() { return state.snap?.groups.find((g) => g.id === state.groupId) || null; }
window.selectGroup = (id) => { state.groupId = id; state.selected = null; render(); };
window.removeGroup = async (id) => { await invoke("remove_group", { id }); if (state.groupId === id) state.groupId = null; };
window.toggleGroup = async (id, on) => { const g = state.snap.groups.find((x) => x.id === id); g.enabled = on; await invoke("update_group", { g }); };

async function setFormat(f) {
  const g = group(); if (!g) return;
  g.format = f; await invoke("update_group", { g });
}
async function exportCsv(path) {
  state.status = "csv -> " + path; // core export happens through workspace; keep simple
}

/* ---------- grid ---------- */
function renderGrid() {
  const view = document.getElementById("view-grid");
  const g = group();
  if (!g) { view.innerHTML = `<p class="small">No poll group. Use “＋” in Poll groups.</p>`; return; }
  const need = regsNeeded(g.format);
  const W = state.snap, B = state.snap.bits;
  let rows = "";
  for (let i = 0; i < g.count; i++) {
    const addr = (g.start + i) & 0xffff;
    const k = key(g.unit, g.fc, addr);
    const isBit = g.fc === 1 || g.fc === 2;
    const disp = state.plcAddr ? addr + 1 : addr;
    if (isBit) {
      const on = W.bits[k] ? 1 : 0;
      const [bg, fg] = colorFor(g.color, on, on);
      rows += `<tr class="row"><td class="addr">${disp}</td><td><span class="cell ${sel(k)}" style="background:${rgb(bg)};color:${rgb(fg)}" onclick="pick('${k}',${g.unit},${g.fc},${addr})">${on ? "1" : "0"}</span></td><td class="name">${on ? "ON" : "off"}</td></tr>`;
    } else {
      const raw = W.words[k];
      const hexRaw = raw === undefined ? "----" : raw.toString(16).toUpperCase().padStart(4, "0");
      let valueCell = `<td></td>`;
      if (i % need === 0) {
        const regs = [];
        for (let o = 0; o < need; o++) { const r = W.words[key(g.unit, g.fc, (addr + o) & 0xffff)]; if (r !== undefined) regs.push(r); }
        if (regs.length === need) {
          const v = Number(fmtValue(g.format, regs).replace(/[^0-9.-]/g, "")) || 0;
          const [bg, fg] = colorFor(g.color, v, regs[0]);
          valueCell = `<td><span class="cell ${sel(k)}" style="background:${rgb(bg)};color:${rgb(fg)}" onclick="pick('${k}',${g.unit},${g.fc},${addr})">${esc(fmtValue(g.format, regs))}</span></td>`;
        } else valueCell = `<td class="small">…</td>`;
      } else valueCell = `<td class="small">(cont)</td>`;
      rows += `<tr class="row"><td class="addr">${disp}</td><td class="raw">${hexRaw}</td>${valueCell}</tr>`;
    }
  }
  const selInfo = state.selected
    ? `<span>sel ${esc(state.selected)}</span>
       <input id="wbuf" size="12" value="${esc(state.writeBuf)}" oninput="state.writeBuf=this.value"/>
       <button onclick="doWrite()">Write</button>` : `<span class="small">click a cell to select it</span>`;
  view.innerHTML = `
    <div class="writebar">${selInfo}
      <span class="sep"></span>
      <label class="small"><input type="checkbox" ${state.plcAddr ? "checked" : ""} onchange="state.plcAddr=this.checked"/> PLC addresses (base 1)</label>
    </div>
    <table class="grid">
      <thead><tr><th>addr</th><th>raw</th><th>value · ${esc(fmtLabel(g.format))}</th></tr></thead>
      <tbody>${rows}</tbody>
    </table>`;
  const wb = document.getElementById("wbuf"); if (wb) wb.oninput = (e) => (state.writeBuf = e.target.value);
}
const sel = (k) => (state.selected === k ? "sel" : "");
window.pick = (k, unit, fc, addr) => { state.selected = { k, unit, fc, addr }; state.writeBuf = ""; render(); };
async function doWrite() {
  const s = state.selected; if (!s) return;
  const g = group();
  if (s.fc === 1 || s.fc === 2) await invoke("write_coil", { unit: s.unit, addr: s.addr, on: !/^(0|false|off)$/i.test(state.writeBuf) });
  else {
    const v = parseInt(state.writeBuf, 10);
    if (!Number.isNaN(v)) await invoke("write_reg", { unit: s.unit, addr: s.addr, value: v & 0xffff });
  }
  state.status = "write sent";
}

/* ---------- chart / traffic / scan ---------- */
function renderChart() {
  const g = group(); const cv = document.getElementById("chart");
  if (!g) return;
  const k = state.selected ? state.selected.k : key(g.unit, g.fc, g.start);
  const v = state.snap.words[k];
  if (v !== undefined) { (state.history[k] = state.history[k] || []).push(Number(fmtValue(g.format, [v, state.snap.words[key(g.unit, g.fc, (g.start + 1) & 0xffff)] || 0]))); if (state.history[k].length > 1000) state.history[k].shift(); }
  const h = state.history[k] || [];
  const ctx = cv.getContext("2d");
  const W = cv.width, H = cv.height;
  ctx.fillStyle = "#101216"; ctx.fillRect(0, 0, W, H);
  if (h.length < 2) { ctx.fillStyle = "#666"; ctx.fillText("waiting for data…", W/2-50, H/2); return; }
  const mn = Math.min(...h), mx = Math.max(...h), span = (mx - mn) || 1;
  ctx.strokeStyle = "#5ac8ff"; ctx.lineWidth = 1.5; ctx.beginPath();
  h.forEach((val, i) => { const x = 10 + (W-20) * i / (h.length-1); const y = H-10 - (H-20) * (val-mn)/span; i ? ctx.lineTo(x,y) : ctx.moveTo(x,y); });
  ctx.stroke();
  ctx.fillStyle = "#aab"; ctx.font = "12px monospace";
  ctx.fillText(`${k}  min=${mn.toFixed(3)} max=${mx.toFixed(3)} last=${h[h.length-1].toFixed(3)} n=${h.length}`, 12, 18);
}
function renderTraffic() {
  const pre = document.getElementById("traffic");
  pre.textContent = (state.snap.traffic || []).map((e) => `${e.t} ${e.dir} ${e.text}`).join("\n");
}
function renderScan() {
  const sc = state.snap.scan;
  document.getElementById("scan").innerHTML = `
    <h3>${sc.kind === 0 ? "Address scan" : "Slave scan"}</h3>
    <div class="small">${sc.running ? `scanning ${sc.cur}/${sc.end}…` : `${sc.found.length} results, ${sc.errors} errors`}</div>
    <table class="grid"><thead><tr><th>address</th><th>value</th></tr></thead>
    <tbody>${sc.found.map((f) => `<tr class="row"><td class="addr">${f[0]}</td><td>${esc(f[1])}</td></tr>`).join("")}</tbody></table>`;
}
function renderTest() { document.getElementById("t-resp").textContent = state.snap.last_response || "(none)"; }

/* ---------- dialogs ---------- */
function show(id, html, wire) {
  const d = document.getElementById(id);
  d.innerHTML = html;
  d.showModal();
  wire && wire(d);
}
const field = (label, inner) => `<label>${label}<br/>${inner}</label>`;

window.dlgConn = () => {
  const c = state.snap.conn;
  show("dlg-conn", `
    <h3>Connection Setup</h3>
    <div class="grid2">
      ${field("Mode", `<select id="c-mode">${["Tcp","Rtu","Ascii","RtuOverTcp","AsciiOverTcp","Udp"].map((m)=>`<option ${c.mode===m?"selected":""}>${m}</option>`).join("")}</select>`)}
      ${field("Host", `<input id="c-host" value="${esc(c.host)}"/>`)}
      ${field("Port", `<input id="c-port" type="number" value="${c.port}"/>`)}
      ${field("Serial port", `<input id="c-ser" value="${esc(c.serial_port)}"/>`)}
      ${field("Baud", `<input id="c-baud" type="number" value="${c.baud}"/>`)}
      ${field("Parity", `<select id="c-par">${["N","E","O"].map((p)=>`<option ${c.parity===p?"selected":""}>${p}</option>`).join("")}</select>`)}
      ${field("Unit", `<input id="c-unit" type="number" value="${c.unit}"/>`)}
      ${field("Timeout ms", `<input id="c-to" type="number" value="${c.timeout_ms}"/>`)}
    </div>
    <div class="actions"><button onclick="this.closest('dialog').close()">Cancel</button><button id="c-ok">Apply &amp; reconnect</button></div>`,
    (d) => {
      d.querySelector("#c-ok").onclick = async () => {
        const conn = { mode: d.querySelector("#c-mode").value, host: d.querySelector("#c-host").value, port: +d.querySelector("#c-port").value,
          serial_port: d.querySelector("#c-ser").value, baud: +d.querySelector("#c-baud").value, data_bits: c.data_bits,
          parity: d.querySelector("#c-par").value, stop_bits: c.stop_bits, timeout_ms: +d.querySelector("#c-to").value, unit: +d.querySelector("#c-unit").value };
        d.close(); await invoke("set_conn", { conn }); state.status = "reconnecting";
      };
    });
};

window.dlgGroup = (isNew) => {
  const g = isNew ? { id: 0, unit: 1, fc: 3, start: 0, count: 10, scan_ms: 1000, enabled: true, format: "U16", color: null, scale: null, name: "Group", chart: true } : group();
  if (!g) return;
  const fmts = ["U16","I16","Hex16","Bin16","Ascii16","U16Swapped"];
  const orders = ["BigEndian","LittleEndian","BigEndianByteSwap","LittleEndianByteSwap"];
  const types = ["U32","I32","Hex32","F32","U64","I64","F64"];
  const allF = [...fmts, ...orders.flatMap((o) => types.map((t) => ({ [t]: o })))];
  show("dlg-group", `
    <h3>${isNew ? "New" : "Read/Write Definition"}</h3>
    <div class="grid2">
      ${field("Name", `<input id="g-name" value="${esc(g.name)}"/>`)}
      ${field("Slave ID", `<input id="g-unit" type="number" value="${g.unit}"/>`)}
      ${field("Function", `<select id="g-fc">${Object.entries(FC).map(([v,l])=>`<option value="${v}" ${g.fc==v?"selected":""}>${l}</option>`).join("")}</select>`)}
      ${field("Address", `<input id="g-start" type="number" value="${g.start}"/>`)}
      ${field("Quantity", `<input id="g-count" type="number" value="${g.count}"/>`)}
      ${field("Scan rate ms", `<input id="g-scan" type="number" value="${g.scan_ms}"/>`)}
      ${field("Format", `<select id="g-fmt">${allF.map((f)=>`<option value='${JSON.stringify(f)}' ${JSON.stringify(g.format)===JSON.stringify(f)?"selected":""}>${fmtLabel(f)}</option>`).join("")}</select>`)}
      ${field("Options", `<label><input id="g-en" type="checkbox" ${g.enabled?"checked":""}/> enabled</label> <label><input id="g-chart" type="checkbox" ${g.chart?"checked":""}/> chart</label>`)}
    </div>
    <div class="actions"><button onclick="this.closest('dialog').close()">Cancel</button><button id="g-ok">${isNew?"Add":"Apply"}</button></div>`,
    (d) => {
      d.querySelector("#g-ok").onclick = async () => {
        const obj = { ...g,
          name: d.querySelector("#g-name").value, unit: +d.querySelector("#g-unit").value, fc: +d.querySelector("#g-fc").value,
          start: +d.querySelector("#g-start").value, count: +d.querySelector("#g-count").value, scan_ms: +d.querySelector("#g-scan").value,
          format: JSON.parse(d.querySelector("#g-fmt").value), enabled: d.querySelector("#g-en").checked, chart: d.querySelector("#g-chart").checked };
        d.close();
        if (isNew) { const ng = await invoke("add_group", { g: obj }); state.groupId = ng.id; }
        else await invoke("update_group", { g: obj });
        state.status = isNew ? "group added" : "group updated";
      };
    });
};

window.dlgColors = () => {
  const g = group(); if (!g) return;
  const cm = g.color || { mode: "Rules", normal_bg: [30,32,38], normal_fg: [220,220,220], auto_fg: true, rules: [], ramp_lo: 0, ramp_hi: 100, levels: 32, palette: "Traffic" };
  const P32 = [];
  for (let i = 0; i < 32; i++) P32.push(palette("Rainbow", i/31));
  const swatches = (target, arr, idx, role) => `<div class="swatches">${P32.map((c, i) => `<span class="swatch" style="background:${rgb(c)}" onclick="setSwatch('${target}',${idx},'${role}',${i})"></span>`).join("")}</div>`;
  const rulesHtml = (cm.rules || []).map((r, i) => `
    <div class="rule">
      <div class="row">
        <input type="checkbox" ${r.enabled ? "checked" : ""} onchange="ruleSet(${i},'enabled',this.checked)"/>
        <b>Rule ${i + 1}</b>
        <select onchange="ruleSet(${i},'op',this.value)">${OPS.map((o) => `<option ${r.op===o?"selected":""}>${o}</option>`).join("")}</select>
        <input type="number" value="${r.value}" onchange="ruleSet(${i},'value',+this.value)"/>
        <input placeholder="label" value="${esc(r.label)}" onchange="ruleSet(${i},'label',this.value)"/>
        <button onclick="ruleDel(${i})">✖</button>
      </div>
      <div class="row"><span class="small">bg</span>${swatches("bg", 0, i, "bg")}<span class="small">fg</span>${swatches("fg", 0, i, "fg")}
        <label class="small"><input type="checkbox" ${r.auto_fg?"checked":""} onchange="ruleSet(${i},'auto_fg',this.checked)"/> auto fg</label></div>
    </div>`).join("");
  show("dlg-colors", `
    <h3>Conditional Colors — ${esc(g.name)}</h3>
    <div class="row">Mode: ${["Rules","Discrete","Smooth"].map((m) => `<button onclick="cmSet('mode','${m}')" ${cm.mode===m?"style='background:var(--accent)'":""}>${m}</button>`).join("")}</div>
    <div id="cm-body"></div>
    <div class="actions"><button onclick="this.closest('dialog').close()">Close</button><button id="cm-ok">Apply to group</button></div>`,
    (d) => {
      window.__cm = cm;
      const body = d.querySelector("#cm-body");
      const draw = () => {
        const m = window.__cm;
        if (m.mode === "Rules") {
          body.innerHTML = `<div class="row small">first matching rule wins</div>${rulesHtml.replace("__X__","")}${rulesHtml}<button onclick="ruleAdd()">＋ add rule</button>`;
          // rebuild rulesHtml fresh (templated above used stale cm) — regenerate:
          body.innerHTML = `<div class="row small">first matching rule wins</div>` +
            (m.rules || []).map((r, i) => `
              <div class="rule">
                <div class="row">
                  <input type="checkbox" ${r.enabled ? "checked" : ""} onchange="ruleSet(${i},'enabled',this.checked)"/>
                  <b>Rule ${i + 1}</b>
                  <select onchange="ruleSet(${i},'op',this.value)">${OPS.map((o) => `<option ${r.op===o?"selected":""}>${o}</option>`).join("")}</select>
                  <input type="number" value="${r.value}" onchange="ruleSet(${i},'value',+this.value)"/>
                  <input placeholder="label" value="${esc(r.label)}" onchange="ruleSet(${i},'label',this.value)"/>
                  <button onclick="ruleDel(${i})">✖</button>
                </div>
                <div class="row"><span class="small">bg</span>${swatchRow(i, "bg")}<span class="small">fg</span>${swatchRow(i, "fg")}
                  <label class="small"><input type="checkbox" ${r.auto_fg?"checked":""} onchange="ruleSet(${i},'auto_fg',this.checked)"/> auto fg</label></div>
              </div>`).join("") + `<button onclick="ruleAdd()">＋ add rule</button>`;
        } else {
          body.innerHTML = `
            <div class="grid2">
              ${field("Range low", `<input type="number" value="${m.ramp_lo}" onchange="cmSet('ramp_lo',+this.value)"/>`)}
              ${field("Range high", `<input type="number" value="${m.ramp_hi}" onchange="cmSet('ramp_hi',+this.value)"/>`)}
              ${field("Levels (2..32)", `<input type="range" min="2" max="32" value="${m.levels}" oninput="cmSet('levels',+this.value);draw()"/> <span>${m.levels}</span>`)}
              ${field("Palette", `<select onchange="cmSet('palette',this.value)">${PALETTES.map((p)=>`<option ${m.palette===p?"selected":""}>${p}</option>`).join("")}</select>`)}
            </div>
            <div class="ramp">${Array.from({length: Math.max(2,m.levels)}, (_,i)=>{const t=i/(Math.max(2,m.levels)-1);const c=palette(m.palette,t);const v=m.ramp_lo+t*(m.ramp_hi-m.ramp_lo);return `<div style="background:${rgb(c)};color:${rgb(contrast(c))}">${Math.round(v)}</div>`;}).join("")}</div>`;
        }
      };
      function swatchRow(i, role) {
        return `<div class="swatches">${P32.map((c, s) => `<span class="swatch" style="background:${rgb(c)}" onclick="ruleSwatch(${i},'${role}',${s})"></span>`).join("")}</div>`;
      }
      window.__drawColors = draw;
      window.cmSet = (k, v) => { window.__cm[k] = v; draw(); };
      window.ruleAdd = () => { window.__cm.rules.push({ op: "Gt", value: 0, value2: 0, set: [], bg: P32[window.__cm.rules.length % 32], fg: [235,235,235], auto_fg: true, label: "", enabled: true }); draw(); };
      window.ruleDel = (i) => { window.__cm.rules.splice(i, 1); draw(); };
      window.ruleSet = (i, k, v) => { window.__cm.rules[i][k] = v; if (k === "auto_fg" || k === "bg") { const r = window.__cm.rules[i]; r.fg = contrast(r.bg); } draw(); };
      window.ruleSwatch = (i, role, s) => { const r = window.__cm.rules[i]; r[role] = P32[s]; if (role === "bg" && r.auto_fg) r.fg = contrast(r.bg); draw(); };
      d.querySelector("#cm-ok").onclick = async () => {
        const ng = { ...g, color: window.__cm }; d.close(); await invoke("update_group", { g: ng }); state.status = "colours applied";
      };
      draw();
    });
};

window.dlgNames = () => {
  const names = state.names || {};
  const rows = Object.entries(names).map(([k, v]) => `<div class="row"><span class="addr">${k}</span><input value="${esc(v)}" onchange="state.names[${k}]=this.value"/><button onclick="delete state.names[${k}];dlgNames()">✖</button></div>`).join("");
  show("dlg-names", `
    <h3>Value Names</h3>
    <div class="row"><input id="n-k" type="number" placeholder="value"/><input id="n-v" placeholder="text"/><button id="n-add">Add</button></div>
    <div style="max-height:320px;overflow:auto;margin-top:8px">${rows}</div>
    <div class="actions"><button onclick="this.closest('dialog').close()">Close</button></div>`,
    (d) => { d.querySelector("#n-add").onclick = () => { state.names = state.names || {}; state.names[+d.querySelector("#n-k").value] = d.querySelector("#n-v").value; dlgNames(); }; });
};

window.dlgLog = () => show("dlg-log", `
  <h3>Log Setup</h3>
  <div class="row"><button id="l-browse">Choose file…</button><span id="l-path" class="small"></span></div>
  <div class="row">Format: <select id="l-fmt"><option>Text</option><option selected>CSV</option></select>
    Policy: <select id="l-pol"><option>StopAtEnd</option><option>RestartAtEnd</option><option selected>Continue</option></select>
    Interval ms <input id="l-ms" type="number" value="1000"/></div>
  <div class="actions"><button onclick="this.closest('dialog').close()">Close</button></div>`,
  (d) => { d.querySelector("#l-browse").onclick = async () => { const p = await invoke("pick_save", { name: "mbrs-log.csv" }); if (p) d.querySelector("#l-path").textContent = p; }; });

window.dlgFn = (code) => {
  const titles = { "05":"05 (0x05) Write Single Coil", "06":"06 (0x06) Write Single Register", "15":"15 Write Multiple Coils", "16":"16 Write Multiple Registers", "22":"22 Mask Write Register", "23":"23 Read/Write Multiple Registers", "08":"08 Diagnostics", "0B":"11 Get Comm Event Counter", "11":"17 Report Server ID", "2B":"43/14 Read Device Identification" };
  const bodies = {
    "05": `Unit <input id="f-u" type="number" value="1"/> Addr <input id="f-a" type="number" value="0"/> Value <select id="f-v"><option value="1">ON</option><option value="0">off</option></select>`,
    "06": `Unit <input id="f-u" type="number" value="1"/> Addr <input id="f-a" type="number" value="0"/> Value <input id="f-v2" type="number" value="0"/>`,
    "15": `Unit <input id="f-u" type="number" value="1"/> Addr <input id="f-a" type="number" value="0"/> Bits <input id="f-vs" value="10101010"/>`,
    "16": `Unit <input id="f-u" type="number" value="1"/> Addr <input id="f-a" type="number" value="0"/> Values <input id="f-vs" value="1 2 3 4"/>`,
    "22": `Unit <input id="f-u" type="number" value="1"/> Addr <input id="f-a" type="number" value="0"/> AND <input id="f-and" type="number" value="65535"/> OR <input id="f-or" type="number" value="0"/>`,
    "23": `Unit <input id="f-u" type="number" value="1"/> Read addr <input id="f-a" type="number" value="0"/> Qty <input id="f-q" type="number" value="4"/> Write values <input id="f-vs" value="1 2"/>`,
    "08": `Unit <input id="f-u" type="number" value="1"/> Sub <input id="f-sub" value="0x0000"/> Data <input id="f-data" type="number" value="0"/>`,
    "0B": `Unit <input id="f-u" type="number" value="1"/>`,
    "11": `Unit <input id="f-u" type="number" value="1"/>`,
    "2B": `Unit <input id="f-u" type="number" value="1"/> Code <input id="f-code" type="number" value="1"/> Object <input id="f-obj" type="number" value="0"/>`,
  };
  show("dlg-fn", `<h3>${titles[code]}</h3><div class="row">${bodies[code]}</div>
    <div class="actions"><button onclick="this.closest('dialog').close()">Cancel</button><button id="f-ok">Send</button></div>
    <pre id="f-resp"></pre>`,
    (d) => { d.querySelector("#f-ok").onclick = async () => {
      const u = +(d.querySelector("#f-u")?.value || 1);
      if (code === "05") await invoke("write_coil", { unit: u, addr: +d.querySelector("#f-a").value, on: d.querySelector("#f-v").value === "1" });
      if (code === "06") await invoke("write_reg", { unit: u, addr: +d.querySelector("#f-a").value, value: +d.querySelector("#f-v2").value & 0xffff });
      if (code === "15") await invoke("write_coils", { unit: u, addr: +d.querySelector("#f-a").value, values: [...d.querySelector("#f-vs").value].filter((c) => c === "0" || c === "1").map((c) => c === "1") });
      if (code === "16") await invoke("write_regs", { unit: u, addr: +d.querySelector("#f-a").value, values: d.querySelector("#f-vs").value.split(/\s+/).map(Number) });
      if (code === "22") await invoke("mask_write", { unit: u, addr: +d.querySelector("#f-a").value, andMask: +d.querySelector("#f-and").value, orMask: +d.querySelector("#f-or").value });
      if (code === "23") await invoke("read_write_multi", { unit: u, readAddr: +d.querySelector("#f-a").value, readQty: +d.querySelector("#f-q").value, writeAddr: +d.querySelector("#f-a").value, values: d.querySelector("#f-vs").value.split(/\s+/).map(Number) });
      if (code === "08") await invoke("diag", { unit: u, sub: parseInt(d.querySelector("#f-sub").value), data: +d.querySelector("#f-data").value });
      if (code === "0B") await invoke("comm_event", { unit: u });
      if (code === "11") await invoke("report_id", { unit: u });
      if (code === "2B") await invoke("device_id", { unit: u, code: +d.querySelector("#f-code").value, obj: +d.querySelector("#f-obj").value });
      state.status = "sent " + titles[code];
    }; });
};

window.dlgScan = (kind) => show("dlg-scan", `
  <h3>${kind === "addr" ? "Address Scan" : "Slave Scan"}</h3>
  <div class="row">
    ${kind === "addr" ? `Unit <input id="s-u" type="number" value="1"/> From <input id="s-a" type="number" value="0"/> To <input id="s-b" type="number" value="19"/> FC <select id="s-fc"><option value="3">03 Holding</option><option value="4">04 Input</option><option value="1">01 Coils</option><option value="2">02 Discrete</option></select>`
      : `From ID <input id="s-a" type="number" value="1"/> To ID <input id="s-b" type="number" value="32"/> Addr <input id="s-u" type="number" value="0"/>`}
    <button id="s-go">Start</button>
  </div>
  <div class="actions"><button onclick="this.closest('dialog').close()">Close</button></div>`,
  (d) => { d.querySelector("#s-go").onclick = async () => {
    if (kind === "addr") await invoke("scan_address", { unit: +d.querySelector("#s-u").value, fc: +d.querySelector("#s-fc").value, start: +d.querySelector("#s-a").value, end: +d.querySelector("#s-b").value });
    else await invoke("scan_slave", { fc: 3, addr: +d.querySelector("#s-u").value, start: +d.querySelector("#s-a").value, end: +d.querySelector("#s-b").value });
    d.close(); state.view = "scan";
  }; });

window.dlgAbout = () => show("dlg-about", `
  <h3>mbrs — Modbus Studio</h3>
  <p>Independent Rust reimplementation. Core (Modbus codec, colour engine, SCADA, logging)
  shared between the egui build and this product front-end.</p>
  <p class="small">Reverse-engineered surface of the original: 44 dialogs, 3 menus, 41 string bundles.</p>
  <div class="actions"><button onclick="this.closest('dialog').close()">Close</button></div>`);

/* ---------- render / poll ---------- */
function render() {
  if (!state.snap) return;
  if (!state.groupId && state.snap.groups.length) state.groupId = state.snap.groups[0].id;
  buildToolbar();
  buildSide();
  document.querySelectorAll(".view").forEach((v) => (v.hidden = true));
  document.getElementById("view-" + state.view).hidden = false;
  if (state.view === "grid") renderGrid();
  else if (state.view === "chart") renderChart();
  else if (state.view === "traffic") renderTraffic();
  else if (state.view === "scan") renderScan();
  else if (state.view === "test") renderTest();
  const st = document.getElementById("status");
  const col = state.snap.state === "Connected" ? "var(--ok)" : (state.snap.state === "Error" ? "var(--err)" : "var(--muted)");
  st.innerHTML = `<span class="dot" style="background:${col}"></span>${esc(state.snap.conn.host)}:${state.snap.conn.port}
    <span>${esc(state.status)}</span>
    <span class="right">ok ${state.snap.stats.ok}  err ${state.snap.stats.err}  timeouts ${state.snap.stats.timeouts}  ${state.snap.stats.last_ms.toFixed(1)} ms</span>`;
}

async function poll() {
  try { state.snap = await invoke("snapshot"); render(); } catch (e) { /* window closing */ }
}
buildMenus();
setInterval(poll, 150);
poll();
