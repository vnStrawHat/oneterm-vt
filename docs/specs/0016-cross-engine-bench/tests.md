# Tests

## vt-bench (root workspace, part of the normal gate)

| ID | Test (in `fixtures.rs` / `fixture_effects_tests.rs`) |
|---|---|
| V1 | existing tests pass unchanged on `FIXTURES` (14); determinism, stop-at-target and `every_fixture_parses_cleanly` also run over `COMPARE_FIXTURES`; names unique across both tables |
| V2 | first unit of each new generator, byte-exact: `cursor_motion` starts `ESC[1;1HA`, `light_cells` = `ESC[?1049h` + `ESC[H` + 7200 x `a`, `short_lines` = `y\r\n`, `scroll_region_small` prefix `ESC[?1049h ESC[1;22r`, `long_osc` unit length 8 000 + framing |
| V3 | `cursor_motion` unit has exactly the ring-cell count at 160x45 and visits every cell once |
| V4 | `combining_unique`: 1 000 consecutive units are pairwise distinct |
| V5 | `csi_mix`: over 10 000 units each of the 7 branches occurs (weights roughly hold) |
| V6 | `unicode_symbols`, `vim_session`: unit equals the vendored file; bare-LF counts are 2 and 77 |
| V7 | effects: fed through `Terminal`, `light_cells` leaves the alt screen full of the last letter; `scroll_region_small` keeps rows 23-45 blank; `long_osc` leaves `osc N` text, no title change |
| V8 | tiers stay at 14: default selection, `all --json` record and `grid --check` see no compare fixture; `select(Some("csi_mix"))` finds it; `fixtures --compare` writes 23 files, without the flag 14 |
| V9 | every corpus fixture that sends `?2026h` also sends `?2026l` later in each unit (vt100 has no flush) |

## compare/harness (unit tests, `cargo test` in `compare/`)

| ID | Test |
|---|---|
| H1 | dump: wide char + spacer gives the char once; empty cell gives one space; trailing spaces trimmed |
| H2 | dump: cursor col `cols` (pending wrap) clamps to `cols - 1` |
| H3 | dump: attr runs merge equal neighbours, skip all-default cells, spacer takes the lead's style |
| H4 | dump: colour mapping `d`, `i<n>` (SGR 31 -> `i1`, 91 -> `i9`), `r<rrggbb>`; flags sorted |
| H5 | dump JSON is byte-stable (field order fixed) and its sha256 matches a pinned value |
| H6 | stats: median and p10/p90 of 15 known samples; zero duration is finite |
| H7 | CLI: unknown flag and missing `--file` exit 2; JSON on stdout only |
| H8 | checkpoints: chunk indices for `n = 1, 3, 4, 513` (each checkpoint at least chunk 1, last = n) |
| H9 | UTF-8 carry (avt adapter helper): a 4-byte char split over 2 and 3 chunks decodes once; an invalid byte becomes one U+FFFD; a valid chunk is fed as a borrowed `&str` (no copy) |
| H10 | verify with a fake engine that buffers until `finish`: every checkpoint dump sees the buffered bytes (finish called before each dump) |
| H11 | dump: a base char with combining marks stored beside it yields base + marks as the cell text |
| H12 | verify order: memory `steady` is sampled after the last `finish` and before the dump builder allocates |

## compare/driver (unit tests)

| ID | Test |
|---|---|
| D1 | status table: every row of verification.md "Statuses" from synthetic verify/timed results |
| D2 | golden plurality: 3 of 5 agree -> golden; 2-2-1 -> no golden, fixture in `ungated.toml`, its grid rows `unverified`, absent from geomean and SVG |
| D3 | accepted.toml: entry matches only on exact (engine, fixture, sha256) |
| D4 | corpus lock: one changed byte -> refusal naming the fixture |
| D5 | rotation: repetition `r` starts at engine `r % 9`; every engine runs once per fixture per repetition |
| D6 | report: markers `≈`, `†`, `≠`, `?`, `n/a`, `fail` are distinct; bold fastest verified; SVG omits diverged and unverified bars; geomean only over the intersection (an engine `ok` on extra fixtures gets no extra terms) |
| D9 | cargo/rustc detection: a fake process list containing `cargo.exe` refuses `run` |
| D7 | machine slug validation and `-2` suffix for a second run the same day |
| D8 | power guard (powercfg behind a function seam): switch failure refuses; restore runs on Ok, Err and panic; a leftover `power-restore.txt` is restored at start; `--keep-power-plan` skips both |

## Cross-language check

X1: the xterm adapter's JS dump code and the Rust `DumpBuilder` produce the same canonical
JSON for 3 small hand-written screens (stored as `compare/harness/tests/dump_cases.json`; the
Rust test reads it, `node adapter.mjs selftest` reads it).

## Manual acceptance on the agreed machine (recorded in `acceptance.md` by the coder)

1. `fetch-engines.sh` then `compare build`: all 9 engines build; versions printed by `info`
   match engines.md.
2. `compare corpus`: 23 files, lock matches.
3. `compare verify` + `compare golden`: goldens for every fixture that reaches plurality; the
   rest in `ungated.toml`.
4. `compare run --machine i7-12700-win11`: wall time recorded (expected 50-80 min, not gated); `.json`, `.md`, `.svg`
   written; `oneterm` is `ok` on every fixture or its divergence is explained.
5. Ctrl-C during `run`: the previous power scheme is active again afterwards.
6. Re-run on the same day: per-fixture medians within each figure's spread for >= 90% of
   `ok` pairs (repeatability).
