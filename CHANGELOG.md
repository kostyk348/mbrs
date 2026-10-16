# Changelog

Notable changes to `mbrs` (Modbus Studio). Version numbers follow the git tags; a tag must match
`version` in `Cargo.toml` — the release workflow refuses to publish otherwise.

## [0.1.0] — 2026-10-16

First tagged release: the desktop application, the shared core and the test suite that runs on a
fresh clone in CI.

### Added

- `src/grid.rs` — virtualised register grid: only the rows inside the viewport are built each frame,
  the table header lives outside the scroll area, and `visible_range()` is unit-tested (a
  100 000-register window costs the same as a 30-row one).
- `src/scan.rs` — address/slave scan extracted from the polling worker into testable core logic
  (`ScanWindow`, `contiguous()`, `summary_text()`), and the Scan tab now reports the result as
  ranges: `found: 0–2, 5, 7–8`.
- `examples/gen_demo.rs` — reproducible demo workspace (`examples/demo.mbw`), tracked on purpose
  because the test suite reads it through `include_str!`.
- CI: `fmt --check`, tests on Linux/Windows/macOS, advisory `clippy`, release packaging on tags
  (`.zip`/`.tar.gz` per target) with `SHA256SUMS.txt` and a tag-versus-`Cargo.toml` guard.
- `LICENSE-MIT`, `LICENSE-APACHE` — the crate has always declared `MIT OR Apache-2.0`; now the
  texts ship with it.

### Fixed

- The test suite no longer fails to compile on a fresh clone (it referenced a demo file that was
  never committed, and `.gitignore` kept it out).
- Scan with `end` below `start` no longer walks the whole address space up to `u16::MAX`.

### Tests

- 23 unit tests in the core (`cargo test`), from 8 before this release.
