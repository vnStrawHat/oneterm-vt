# Steps, files, acceptance, gate, commits

Order: smallest blast radius first, dependencies respected (S4 extends S2's run, S8 uses S7's
`attach_marks` and S2's ASCII path). Gate green and one commit per step.

## How to measure (every threshold below)

```bash
cargo build -p oneterm-vt-bench --release
exe=$(cygpath -w "$CARGO_TARGET_DIR/release/vt-bench.exe")
cmd //c start "" //b //wait //high //affinity 10 "$exe" grid --fixture NAME --mib 32
```

Pinned to logical CPU 4, high priority, median of 5. Before a step, record the step's fixtures
plus `heavy_sgr`, `dense_cells`, `tui_redraw`, `sixel`, `hyperlinks`; after it, the same list.
A threshold is an absolute MiB/s floor (about 0.8 x the prototype); every other fixture of the
list stays >= 0.95 x its before value. Numbers go into `acceptance.md`.

## Steps

| Step | Item | Files (crates/vt/src unless noted) | Threshold after the step |
|---|---|---|---|
| S1 | 1 | `intern.rs`, `intern_tests.rs`, `terminal/sgr.rs`, `terminal.rs` (`live_ids`, `interner_mut` doc), `terminal/graphics.rs` (rename), `grid/terminal_grid.rs`, `grid/screen.rs`, `grid/row.rs`, `grid/row_tests.rs`, `events.rs` (doc), `terminal/terminal_tests.rs` | compare verify: `csi_mix` ok; `heavy_sgr`, `dense_cells` >= 0.95 x before |
| S2 | 4 | `cell.rs`, `cell_tests.rs`, `grid/row.rs`, `grid/row_tests.rs`, `grid/screen/print.rs`, `grid/screen/screen_tests.rs`, `grid/terminal_grid.rs`, `terminal/print.rs` | `plain_ascii` >= 330, `long_lines` >= 440, `scrolling` >= 460, `light_cells` >= 540 |
| S3 | 4 | `parser/ground.rs`, `parser/parser_tests.rs` | `plain_ascii` >= 500, `long_lines` >= 680, `scrolling` >= 650, `light_cells` >= 1,000 |
| S4 | 6 | `grid/screen/print.rs`, `grid/terminal_grid.rs`, `terminal/print.rs`, `parser/ground.rs`, tests | `cjk_wide` >= 185, `unicode_symbols` >= 190, `tui_redraw` >= 260 |
| S5 | 5 | `events/batch.rs`, `events.rs`, `events/event_tests.rs`, `tests/feed_limits.rs`, `grid/row.rs`, `grid/row_tests.rs` | `short_lines` >= 48 |
| S6 | 3 | `grid/screen.rs`, `grid/screen/screen_tests.rs` | `scroll_region` >= 220, `scroll_region_small` >= 21 |
| S7 | 2 | `intern.rs`, `intern_tests.rs`, `grid/row.rs`, `grid/screen.rs`, `grid/terminal_grid.rs`, `grid/screen/print.rs`, `terminal/print.rs`, `terminal/terminal_tests.rs`, `tests/feed_stats.rs` | `combining_unique` >= 14 |
| S8 | 7 | `terminal/print.rs`, `width.rs`, `width_tests.rs`, `terminal/terminal_tests.rs` | `grapheme_clusters` >= 45 |
| S9 | - | user docs below, `acceptance.md`, compare run | all 21 goldens ok; `vt-bench grid --check` passes |

`tests/feed_chunking.rs` gains its pieces in S2 (ASCII word), S3 (`\x7f`, `\u{85}`), S1
(`48:2` SGR), S6 (region, alt screen) and S7 (mark run).

## User docs (in the step that changes the behaviour)

| File | Step | Change |
|---|---|---|
| `docs/user/limits.md` | S1, S5, S7 | styles row (styles freed like extras, swept when the table is full); event row (32,768 events, 4,096 parameter spans); grapheme row ("swept after a feed once it doubled; keeps at most half of each cap, newest rows first; a refused or dropped cluster keeps its base glyph"); `grapheme_table_exhausted` row ("clusters kept without their new marks because the arena was full") |
| `docs/user/cells-and-styles.md` | S1, S7 | "Ids already issued never change meaning, except grapheme ids at a sweep" -> also "an unused style or extras id may be reused"; the sweep section states the half-cap rule and that a dropped id's cells keep their first scalar in the terminal |

`docs/user/api-map.md`: nothing (no new public item). `docs/perf/README.md`: nothing (targets
and records unchanged; the next release records the new numbers).

## Gate (after every step)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --all-features               # as CI: vt-paranoid widens the integrity walk
```

No `unsafe`, no new dependency, no new `pub` item: the public API snapshot (`scripts/public-api.sh
check`, CI) must not change. `GraphemeRemap`'s fields are private, so its new representation is
not part of the snapshot.

## Commits (docs/agents/commits.md)

`docs(spec): engine hot paths after the first cross-engine run` first, then one per step, the
subject naming the fixture and the measured before -> after:

| Step | Subject (numbers from acceptance.md) |
|---|---|
| S1 | `fix(intern): reclaim unused style ids so csi_mix keeps its colours` |
| S2 | `perf(grid): write printable ASCII runs in bulk, plain_ascii A -> B MiB/s` |
| S3 | `perf(parser): scan printable ASCII in 16-byte blocks, scrolling A -> B MiB/s` |
| S4 | `perf(grid): place wide glyphs in the print run, cjk_wide A -> B MiB/s` |
| S5 | `perf(events): keep 32768 events between feeds, short_lines A -> B MiB/s` |
| S6 | `perf(grid): rotate region rows as one slice, scroll_region A -> B MiB/s` |
| S7 | `perf(intern): one intern per mark run, half-cap sweep, combining_unique A -> B MiB/s` |
| S8 | `perf(terminal): bulk ASCII under ?2027, grapheme_clusters A -> B MiB/s` |
| S9 | `chore(bench): compare run i7-12700-win11 <date>` (with `acceptance.md`) |

Bodies end with `Spec: docs/specs/0017-engine-hot-paths`. S7's body lists the behaviour
changes from graphemes.md.
