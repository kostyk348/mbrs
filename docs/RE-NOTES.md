# Reverse-engineering notes — Modbus Poll 13.2.1 (trial, Windows x64)

## Sample

- Artifact: `ModbusPollSetup64Bit.exe` (download: `https://www.modbustools.com/download/ModbusPollSetup64Bit.exe`)
- Size: 4,539,736 bytes
- SHA-256: `446af3ba1b49a926671cfe1c018cb0058d49faf1cbb0a41a28cac58498b6ab77`
- Outer: PE32 GUI, **Nullsoft (NSIS)** self-extracting installer (extractable with 7-Zip).
- Payload entry: `mbpoll.exe`
  - PE32+ x86-64, GUI, 6 sections, SHA-256 `13d8770900d6cb7125a54d6fc3d9aa933de61a07ae71bc9c11efd0640b303a3a`
  - `.text` 6.6 MB (code), `.pdata`, `.fptable`, `.rsrc`

## Toolkit fingerprint

Not Delphi, not .NET. MSVC **C++ with RTTI** (MFC/ATL/Win32):

- `.?AVCModbusData@@`, `.?AVCModbusTCPSecurity@@`, `.?AVCModbus...`
- MFC: `CWinApp`, `CFrameWnd`, `CMDIChildWnd`, `CSplitButton`, `CListCtrl`, `CFileDialog`
- Grid control: `CGridCtrl`, `CGridCell`, `CGridDefaultCell`, `CInPlaceEdit`
- Charting: `CChartCtrl`, `CChartAxis`, `CChartLineSerie`, `CChartCandlestickSerie`
- Parsing/persistence: `tinyxml2` (`XMLDocument`, `XMLElement`)
- COM/OLE Automation: `mbpoll.tlb` (`COleObjectFactory`, dispatch impl)

## Feature surface (from classes + bundled user manual)

- Connections: RTU/ASCII serial, Modbus/TCP, Modbus/TCP Security, RTU/ASCII-over-TCP,
  UDP, RTU-over-UDP.
- Functions: 01,02,03,04,05,06,08,0B,0F,10,11,16,17,22,23,43/14.
- Classes: `CAddressScan`, `CSlaveScan`, `CTestCenter`, `CCommunication`,
  `CErrorCounter`, `CExcelLogSetup`, `CReadDeviceIdentification`, `CScaling`,
  `CValueNames`, `CChart`.
- Dialogs `CMB*`: write coil/register/float/int/bin, mask write, read/write
  multiple, force multiple coils, preset multiple registers, report slave id.

## Conditional colours — the exact baseline

Manual §10 + OLE API §19.9 (`ColorsSetColors`, `ColorsSetRules`):

- Structure holds **6 colours**: Normal (bg,fg), Rule 1 (bg,fg), Rule 2 (bg,fg).
- 7 operators: not used, equal, greater, less, ≥, ≤, **and** (bit test, 16-bit only).
- Rule 1 takes precedence over Rule 2.
- → effectively **3 visual states / 3 background colours**, each a single RGB.

`mbrs` replaces this with N ordered rules and a 32-level discrete / smooth ramp.

## Method

- Static bytes: `g-tools` (mmap/SIMD) — sections, entropy, strings, RTTI.
- Deep analysis: REA + Ghidra headless (`rea_open_binary` on `mbpoll.exe`).
- Behavioural reference: bundled `mbpoll-user-manual.html`, `ReadMe.txt`.
