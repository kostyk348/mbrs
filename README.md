# mbrs — Modbus Studio (Rust)

A from-scratch Modbus master / monitor written in Rust, inspired by the feature
surface of *Modbus Poll* (Witte Software) but with a far stronger value→colour
engine and a SCADA-style dashboard.

> Independent reimplementation. No code from Modbus Poll is used or included.
> The original is proprietary (MFC/C++, x64, COFF/PE). See `docs/RE-NOTES.md`.

## Features

### Protocol (implemented from scratch)
- Framings: **Modbus/TCP** (MBAP), **TCP Security-ready seam**, **RTU** (serial),
  **ASCII** (serial), **RTU/ASCII over TCP**, **UDP**.
- Function codes: `01 02 03 04 05 06 08 0B 0F 10 11 16 17 22 23 43/14`.
- Own CRC-16/MODBUS and LRC, exception decoding, transaction-id checking.
- Unit-tested codec (`crc16`, `lrc`, bit packing).

### Display formats
28 formats: 16-bit signed/unsigned/hex/bin/ASCII/byte-swap, 32- and 64-bit
signed/unsigned/float/double with all four word/byte orders (AB CD, CD AB,
BA DC, DC BA), plus **linear scaling** and **value names** (0=Ready, 1=Running…).

### Colour engine — the differentiator
Modbus Poll offers only `Normal + Rule 1 + Rule 2` (~3 visual states, one colour
each). `mbrs` offers:
- **unbounded ordered rules**, first match wins — `eq ne gt ge lt le`,
  bitmask `has-all-bits` / `has-any-bit`, `in-range`, `one-of`;
- a **32-colour palette** for one-click binding;
- **Discrete mode**: maps a value range onto exactly *N* distinct colours
  (N up to 32) — “paint a cell in ≥32 colours depending on value”;
- **Smooth mode**: continuous ramp (Traffic / Rainbow / Viridis / Heat / Cool / Greys);
- live preview of the resulting colour band.

### SCADA dashboard
Bind tiles (Value / Lamp / Bar / Gauge) to registers and let the colour engine
drive them — so a human never has to read raw register tables.

### Extras
- Strip chart with min/max/last and history buffer.
- Communication-traffic log (with error highlighting).
- JSON workspace save/load; CSV export.
- Per-group scan rate, enable/disable, error & timeout counters.

## Build & run
```sh
cargo run --release
```
Test suite:
```sh
cargo test
```

## Layout
```
src/modbus.rs   framings, CRC/LRC, transport, client helpers
src/formats.rs  28 display formats + scaling/parse
src/colors.rs   rules + 32-level/smooth ramps + palettes
src/scada.rs    tile model + auto-layout
src/store.rs    shared snapshot + polling worker thread
src/workspace.rs project (JSON) persistence
src/app.rs      egui UI: menus, grid, colour editor, SCADA canvas, chart, traffic,
                scan, test center, full dialog set + shortcuts
src/names.rs    value names + binary (bit) names
src/logging.rs  text/CSV logging with Stop/Restart/Continue policies
docs/UI-RE.md   reverse-engineered Modbus Poll interface map
docs/COLOR-ENGINE-RE.md  instruction-level reverse of the colour engine
docs/COMPARISON.md       Modbus Poll vs mbrs
```

## Status
Feature parity pass done against the reversed interface (`docs/UI-RE.md`):

- menu bar mirrors the original (File/Connection/Functions/Setup/Display/View/Help);
- shortcuts match: F3/F4/F5/F8/F11/F12, Alt+F5..F8, Alt+A/R/L/O,
  Alt+Shift+S/U/H/A/B/N/C/F, Ctrl+Shift+S/V, Ctrl+N/O/S;
- dialogs: Connection Setup, Read/Write Definition, Cell Colors, Scaling,
  Value Names, Binary Names, Log Setup, Excel Log, Series Settings,
  Error Counters, Advanced, Modbus/TCP Security, About;
- function dialogs: 05/06/15/16 writes, 22 Mask Write, 23 Read/Write Multiple,
  08 Diagnostics, 0B Comm Event Counter, 11 Report Server ID, 43/14 Device ID;
- Address Scan, Slave Scan, Test Center (arbitrary PDU), traffic log;
- workspace JSON persists groups/tiles/names/log; SCADA tiles + 32-level colour.

Known gaps: Modbus/TCP Security TLS backend (seam only), Excel-com/OLE,
print/print-preview, candlestick chart series.
