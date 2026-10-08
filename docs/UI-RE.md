# Modbus Poll — interface reverse (`.rsrc`)

Extracted from `mbpoll.exe` (.rsrc, 105 984 bytes) with a hand-written PE resource
parser. Nothing here comes from the manual — it is the shipped UI resource table.

## Resource inventory

| type | count | notes |
|---|---|---|
| DIALOG | 44 | all dialogs, with full control trees |
| MENU | 3 | File / Format / (context) menu templates |
| STRING | 41 | 16-entry bundles → command help strings |
| ACCEL | 2 | accelerator tables |
| AFX_DIALOG_LAYOUT | 40 | MFC dialog-layout blobs |
| TYP240 | 9 | **owner-draw combo-box item lists** (function/baud/format/…) |
| TYP241 | 2 | owner-draw state bitmaps |
| ICON / GRPICON | 3 / 2 | app + document icons |
| CURSOR / GRPCURSOR | 18 / 16 | chart cursors etc. |
| BITMAP | 4 | toolbar/glyphs |
| VERSION | 1 | see below |
| MANIFEST | 1 | UAC/comCtl32 manifest |

Version: **Modbus Poll 13.2.1.2558**, CompanyName "Witte Software",
Comments "Written by Brian Witte", InternalName/OriginalFilename `mbpoll.exe`.

## Dialogs (caption — item count)

```
About Modbus Poll              9     Cell Colors                   27
Read/Write Definition         38     Real Time Charting            27
Connection Setup              33     Save/Copy Series              22
06 (0x06) Write Single Reg.   21     Series Settings               98
Write Single Register Binary  48     Binary Names                   5
15 (0x0F) Write Multiple Coils 15    43/14 Read Device Identification 19
Enter Binary Value            31     Hint                           5
Log Setup                     33     Address Scan                  14
This is an unregistered copy   8     Set Slave ID for all Windows   4
Communication Traffic          8     Save/Copy Address Scan         8
Test Center                   13     08 (0x08) Diagnostics          18
22 (0x16) Mask Write Register 57    11 (0x0B) Get Comm Event Counter 15
Advanced                      10     Modbus/TCP Security            8
Error Counters                 2     Strings                        6
Excel Log Setup               18     Slave Scan                    10
Scaling                       13     Value Names                    6
05 (0x05) Write Single Coil   21     Set Scan Rate for all Windows  5
16 (0x10) Write Multiple Regs 16     New                            5
Enter Value                    4     (3 untitled templates)     10/7/0
Write Integer                 17
23 (0x17) Read/Write Mult.    23
17 (0x11) Report Server ID     6
Write Float                   17
```

(The register-write dialogs are 5 near-identical variants: Integer / Float /
Binary, each a separate template.)

## Menu commands (flat, cmd-id + label)

```
File      New Ctrl+N / Open… Ctrl+O / Close / Save Ctrl+S / Save As…
          Export to CSV… / Export to Modbus Slave… / Print Ctrl+P / Print Preview
          Recent: Open/Print Setup… / Exit
Edit      Cut Ctrl+X / Copy Ctrl+C / Paste Ctrl+V / Select All Ctrl+A
Connection  Connect… F3 / Disconnect F4 / Enable / Disable / Quick Connect F5
Functions 05 Write Single Coil Alt+F5 / 06 Write Single Register Alt+F6 /
          08 Diagnostics / 11 Get Comm Event Counter / 15 Write Multiple Coils Alt+F7 /
          16 Write Multiple Registers Alt+F8 / 22 Mask Write Register /
          23 Read/Write Multiple Registers / 43/14 Read Device Identification
Setup     Read/Write Definition F8 / Set Slave ID for all… Shift+F8 /
          Log… Alt+L / Logging Off Alt+O / Reset Counters F12 / Reset All Shift+F12 /
          Address Scan Alt+A / Slave Scan… / Communication… / Use as Default
Display   Signed Alt+Shift+S / Unsigned Alt+Shift+U / Hex Alt+Shift+H /
          ASCII-Hex Alt+Shift+A / Binary Alt+Shift+B / Show Binary Names… Alt+Shift+N /
          Big-endian / Little-endian / Big-endian byte swap / Little-endian byte swap /
          Real Time Charting… Alt+R / Series 1 / Series 2 / Unlink All /
          Colors… Alt+Shift+C / Font… Alt+Shift+F / Scaling… Ctrl+Shift+S /
          Error Counters… F11 / Resize all Windows
View      Toolbar / Status Bar / Always On Top
Help      Help Topics / User manual / E-mail support / Home Page / About… (cmd 0xE140)
```

## Owner-draw lists (TYP240) — exact dropdown contents

* **Function**: `01 Read Coils (0x)`, `02 Read Discrete Inputs (1x)`,
  `03 Read Holding Registers (4x)`, `04 Read Input Registers (3x)`,
  `05 Write Single Coil`, `06 Write Single Register`, `15 Write Multiple Coils`,
  `16 Write Multiple Registers`.
* **Baud**: 300, 600, 1200, 2400, 4800, 9600, 14400, 19200, 38400, 56000, 57600,
  115200, 128000, 153600, 230400, 256000, 460800, 921600, then
  `Data bits` (7/8), `Parity` (None/Odd/Even), `Stop bits` (1/2).
* **Format**: `Signed`, `Unsigned`, `Hex`, `Binary`,
  `Int32`/`UInt32`/`Int64`/`UInt64`/`Float`/`Double` × {Big-endian, Little-endian,
  Big-endian byte swap, Little-endian byte swap}.
* **Logging**: `Stop at end`, `Restart at end`, `Continue`, `Auto Panning`.
* **Device ID**: `01 Basic Device Identification`, `02 Regular Device
  Identification`, `03 Extended Device Identification`,
  `04 One Specific Identification Object`.

## Coverage vs mbrs

| Modbus Poll UI | mbrs today |
|---|---|
| Connection Setup | `dialog_conn` ✓ |
| Read/Write Definition | poll-group side panel + group editor (partial) |
| Cell Colors | `dialog_colors` — superset (N rules / 32 levels) ✓ |
| Scaling | `dialog_scale` ✓ |
| Value Names | `dialog_names` ✓ |
| Communication Traffic | Traffic view ✓ |
| Real Time Charting / Series Settings / Save-Copy Series | strip chart ✓ / series editor ✗ |
| Log Setup / Excel Log Setup | CSV export only ✗ |
| 05/06/15/16/22/23 write dialogs | protocol layer ✓ / dialogs ✗ |
| 08 / 0B / 11 / 43-14 dialogs | protocol layer ✓ / dialogs ✗ |
| Address Scan / Slave Scan | ✗ |
| Error Counters | stats line (partial) |
| Test Center | ✗ |
| Binary Names | ✗ |
| Modbus/TCP Security | ✗ (TLS seam only) |
| Advanced / Hint / About / trial nag | About ✓, rest ✗ |

## Method

`objdump -h` → section VMAs; manual PE data-directory walk → `.rsrc` root;
recursive resource directory traversal; `DLGTEMPLATE`/`DLGTEMPLATEEX`,
`MENUITEMTEMPLATE` and `STRINGTABLE` decoded by hand. Script kept in-tree as a
reference under `docs/` provenance (not shipped in the binary).
