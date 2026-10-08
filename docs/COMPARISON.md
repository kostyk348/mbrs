# Modbus Poll (binary) vs mbrs (Rust) — code-level comparison

Evidence is taken from the shipped `mbpoll.exe` (strings, RTTI, sections) and its
published OLE/automation contract, then contrasted with this crate's source.

Binary under analysis: `mbpoll.exe` v13.2.1 Build 2558, PE32+ x86-64, MFC/C++
(`.?AVCModbusData@@`, `.?AVCModbusTCPSecurity@@`, `tinyxml2`, OpenSSL).
Version string found: `Modbus Poll - 64 Bit. Version %d.%d.%d, Build %d`.

---

## 1. Language & architecture

| | Modbus Poll (real) | mbrs |
|---|---|---|
| Language | C++ (MSVC C++/MFC/ATL, RTTI) | Rust 2021 |
| UI toolkit | MFC (`CFrameWnd`, `CMDIFrameWnd`) + GDI+ / Direct2D | `egui`/`eframe` (OpenGL) |
| Grid | `CGridCtrl` / `CGridCell` / `CInPlaceEdit` | hand-painted cells in `app.rs` |
| Chart | `CChartCtrl` (line/bar/candle) | `draw_chart()` strip chart |
| XML | `tinyxml2` (`.mbp`/`.mbw`) | `serde_json` (`*.mbw`) |
| TLS | OpenSSL (for Modbus/TCP Security) | not yet wired (TLS seam planned) |
| Automation | OLE/COM (`mbpoll.tlb`) | workspace file + CLI arg |

## 2. Modbus protocol surface

Real function codes (from classes + manual §2.2):
`01 02 03 04 05 06 08 0B 0F 10 11 16 17 22 23 43/14`.
mbrs implements the **same set** — see `src/modbus.rs` (`FC_*` constants, `Transport`
helpers `read_bits/read_regs/write_single_coil/write_single_reg/write_regs/write_coils/mask_write_reg/read_write_regs/report_server_id/read_device_identification`).

Real connection modes (OLE `Connection` property):

| Code | Modbus Poll | mbrs `Mode` |
|---|---|---|
| 0 | Serial port (RTU/ASCII via `Mode`) | `Rtu`, `Ascii` |
| 1 | Modbus TCP/IP | `Tcp` |
| 2 | Modbus UDP/IP | `Udp` |
| 3 | ASCII/RTU over TCP/IP | `RtuOverTcp`, `AsciiOverTcp` |
| 4 | ASCII/RTU over UDP/IP | (planned) |
| 5 | Modbus TCP/Security | (TLS planned) |

Real read limits (OLE): ReadCoils/DiscreteInputs 1..2000, Holding/Input 1..125,
ScanRate 0..3 600 000 ms. mbrs enforces the same Modbus PDU limits implicitly and
uses a per-group `scan_ms`.

Real serial defaults: baud 9600, 8 data bits, parity 2=Even, 1 stop bit — mbrs
`ConnConfig::default()` uses 9600/8/`'N'`/1 (parity selectable).

## 3. Display formats — exact string match

The binary contains these format labels verbatim:

```
Int32 Big-endian            Int32 Big-endian byte swap
Int32 Little-endian         Int32 Little-endian byte swap
UInt32 ... (same 4)         Int64 / UInt64 ... (same 4)
Float Big-endian ... (4)    Double Big-endian ... (4)
int16  uint16  ASCII  Binary  HexMode
```

mbrs `ValueFormat` (`src/formats.rs`) reproduces exactly this matrix:
`U16/I16/Hex16/Bin16/Ascii16/U16Swapped` + `{U32,I32,Hex32,F32,U64,I64,F64}` each
parameterised by `WordOrder::{BigEndian, LittleEndian, BigEndianByteSwap, LittleEndianByteSwap}`.
`regs_from_u32`/`bytes()` implement the four byte orders the same way the labels imply.

## 4. Colour engine — where the code actually diverges

This is the core finding.

### Modbus Poll (real code contract)

From the OLE section of the manual and the literal strings `conditional1`,
`conditional2` in the binary:

- Internal structure holds **exactly 6 colours**:
  `0 Normal bg, 1 Normal fg, 2 Rule 1 bg, 3 Rule 1 fg, 4 Rule 2 bg, 5 Rule 2 fg`
  (OLE `ColorsSetColors(ID 0..5, R, G, B)`).
- **Exactly 2 rules.** `ColorsSetRules(Rule1, Rule2)`.
- **Rule 1 takes precedence over Rule 2.**
- **7 operators only** — 0 not used, 1 equal, 2 greater, 3 less, 4 ≥, 5 ≤,
  6 "and" (bit test) and the "and" form is **16-bit integers only**, value in hex,
  true when *any* bit is set in both.
- Net effect: **3 background states** (Normal / Rule1 / Rule2), one solid RGB each.

### mbrs (`src/colors.rs`)

- `ColorMap.rules: Vec<ColorRule>` — **unbounded**, evaluated top-down, first match wins.
- `ColorOp` has **11 operators**: `Eq, Ne, Gt, Ge, Lt, Le, BitAll, BitAny, Range, InSet`
  (+`NotUsed`) — superset of the original 7, and the bit ops work on the raw register.
- **32-colour palette** `PALETTE32` for one-click binding.
- New colour modes the original has no concept of:
  - `ColorMode::Discrete` — maps `[ramp_lo, ramp_hi]` onto exactly `levels` (2..=32)
    distinct ramp colours → **≥32 colours from one value**.
  - `ColorMode::Smooth` — continuous ramp over `Palette::{Traffic,Rainbow,Viridis,Heat,Cool,Greys}`.
- The unit test `discrete_produces_32_distinct` asserts ≥32 distinct colours.

So a direct A/B on the feature the task called out:

```
Modbus Poll: rule(s) -> one of 2 fixed RGB pairs     (max 3 background colours)
mbrs:        rule(s) -> 32-swatch palette, or
             value   -> 32-level discrete ramp, or
             value   -> smooth ramp                  (>=32 colours, one line of config)
```

### SCADA

Modbus Poll has **no SCADA tile layer** — you read a coloured register grid.
mbrs adds `src/scada.rs`: `TileKind::{Value,Lamp,Bar,Gauge}` bound to a register,
driven by the same `ColorMap`, with `autolayout()`. An operator sees a dashboard,
not a table.

## 5. What mbrs still lacks (honest gap list)

- Modbus/TCP Security (TLS) — original uses OpenSSL; mbrs has the transport seam only.
- OLE/COM automation endpoint (`.tlb`) — mbrs uses a JSON workspace + CLI instead.
- Excel logging, Test Center, Address/Slave scan, candlestick chart series.
- Windows-native file dialogs / serial enumeration (mbrs uses text fields; `serialport` enumerates).

## 6. REA / tooling status

`rea_open_binary` on `mbpoll.exe` succeeded (sha256 `13d8770900d6cb7125a54d6fc3d9aa933de61a07ae71bc9c11efd0640b303a3a`),
but every subsequent REA MCP call (`binary_overview`, `search_procedures`,
`search_strings`) fails with:

```
MCP error -32602: Structured content does not match the tool's output schema:
data must have required property 'result' / 'evidence_id' / 'evidence'
```

i.e. the local bridge patch updated `open_binary`'s schema but not the other
operations'. Until that's fixed, function-level decompilation is done manually
(Ghidra headless) and the evidence above is from strings/RTTI + the automation
contract in the bundled manual.
