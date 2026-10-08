# Modbus Poll — conditional-colour engine, at instruction level

Static reverse of `mbpoll.exe` (v13.2.1 Build 2558, PE32+ x86-64, MFC/C++).
No symbols — function boundaries taken from the PE `.pdata` exception directory
(29 493 functions), string xrefs via RIP-relative operand decoding (capstone).

## How the functions were found

1. Parse `.pdata` (VA `0x1408c5000`, 29 493 `RUNTIME_FUNCTION` entries) → exact
   `[begin,end)` for every function.
2. Byte-search `.rdata` for the config strings (`conditional1`, `conditional2`,
   `compare1`, `Colors`, …) and compute their VAs.
3. Disassemble every function range and keep `lea reg,[rip+disp]` / memory operands
   whose resolved target equals a config-string VA.

Result: exactly two functions reference `conditional1`/`conditional2`:

| VA | role | caller |
|---|---|---|
| `0x14011a870` | Conditional-colour **deserialize** (XML → struct) | `0x140192250` (document load) |
| `0x14011acb0` | Conditional-colour **serialize** (struct → XML) | `0x140192c40` (document save) |

## The structure (from the loader `0x14011a870`)

```
lea rdx,[rip+...]        ; "Text"        -> call readColorAttr(rdi+0x08)
lea rdx,[rdi + 0xc]      ; "Back"        -> call readColorAttr(rdi+0x0c)
lea rdx,[rdi + 0x10]     ; "TextRule1"   -> call readColorAttr(rdi+0x10)
lea rdx,[rdi + 0x14]     ; "BackRule1"   -> call readColorAttr(rdi+0x14)
lea rdx,[rdi + 0x18]     ; "TextRule2"   -> call readColorAttr(rdi+0x18)
lea rdx,[rdi + 0x1c]     ; "BackRule2"   -> call readColorAttr(rdi+0x1c)
...
call <attr as double>    ; conditional1 -> movsd [rdi+0x20]   (rule 1 value)
call <attr as double>    ; conditional2 -> movsd [rdi+0x28]   (rule 2 value)
mov r9d, 6               ; clamp enum to 0..6
call <attr int>          ; compare1     -> mov [rdi+0x30]      (rule 1 operator)
mov r9d, 6
call <attr int>          ; compare2     -> mov [rdi+0x34]      (rule 2 operator)
mov [rdi+0x38]           ; "cnc"        (extra flag)
<wstring>  [rdi+0x40]    ; "Name"
<ptr 0x5c bytes> [rdi+0x48]  ; sub-record: 5 dwords + 8 bytes + 32 wchars
```

### Field layout

| off | type | meaning |
|---|---|---|
| 0x08 | u32 | normal text colour |
| 0x0c | u32 | normal back colour |
| 0x10 | u32 | rule 1 text colour |
| 0x14 | u32 | rule 1 back colour |
| 0x18 | u32 | rule 2 text colour |
| 0x1c | u32 | rule 2 back colour |
| 0x20 | f64 | rule 1 condition value |
| 0x28 | f64 | rule 2 condition value |
| 0x30 | u32 | rule 1 operator (0..6) |
| 0x34 | u32 | rule 2 operator (0..6) |
| 0x38 | u32 | `cnc` flag |
| 0x40 | wstring | cell name |
| 0x48 | ptr | 92-byte sub-record (5×u32, 8 bytes, 32×u16) |

The per-cell **font** is a separate `<Font>` element with the Windows `LOGFONT`
fields (`lfHeight lfWidth lfEscapement lfOrientation lfWeight lfItalic
lfUnderline lfStrikeOut lfCharSet lfOutPrecision lfClipPrecision lfQuality
lfPitchAndFamily lfFaceName`).

## The on-disk schema (attribute names recovered from the binary)

```xml
<Cell Name="TEMP">
  <Colors Text="16777215" Back="0"
          TextRule1="..." BackRule1="..."
          TextRule2="..." BackRule2="..."/>
  <Compare compare1="2" compare2="3" conditional1="0" conditional2="0" cnc="..."/>
  <Font lfHeight="..." ... lfFaceName="..."/>
</Cell>
```

## Conclusion — exactly the manual, now from code

* **6 colours total**: normal Text/Back + Rule1 Text/Back + Rule2 Text/Back.
* **2 rules**, each with **one** condition value (f64) and **one** operator.
* Operator enum clamped to **0..6** (`mov r9d,6` before the getter) — the 7
  operators: not used, equal, greater, less, ≥, ≤, and.
* No ramp / N-level mapping anywhere in this record.

## Contrast with mbrs

| | Modbus Poll (`0x14011a870`) | mbrs `ColorMap` |
|---|---|---|
| rule count | 2 (fixed fields 0x30/0x34) | `Vec<ColorRule>`, unbounded |
| colours | 6 (fields 0x08..0x1c) | 32-swatch `PALETTE32` |
| operators | 7, enum 0..6 | 11 (`ColorOp`) |
| op test | equal/greater/less/≥/≤/bit-and | + `Ne`, `BitAll`, `BitAny`, `Range`, `InSet` |
| value→colour | 2 boolean rules | + `Discrete` (2..=32 levels) + `Smooth` ramp |
| persistence | tinyxml2 `<Colors>/<Compare>` | `serde_json` `ColorMap` |

The "≥32 colours from a value" requirement is something the original data model
**cannot express**: `BackRule2` is a single RGB, and there is no field for a level
or ramp. mbrs' `ColorMode::Discrete` fills exactly that gap (unit test
`discrete_produces_32_distinct`).
