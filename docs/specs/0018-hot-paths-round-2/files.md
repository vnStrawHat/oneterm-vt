# Steps, files, acceptance, gate, commits

Order: gain per complexity, smallest blast radius first. Steps touch disjoint functions, so each
is green on its own; S8 is last because it is the owner's decision (README) and can be
dropped without touching the others. The prototype measured them in this order.

## How to measure (every threshold below)

```bash
cargo build -p oneterm-vt-bench --release
exe=$(cygpath -w "$CARGO_TARGET_DIR/release/vt-bench.exe")
cmd //c start "" //b //wait //high //affinity 10 "$exe" grid --fixture NAME --mib 32
```

Pinned to logical CPU 4, high priority, median of 5. Before a step, record the step's fixtures
plus the guard list `heavy_sgr`, `dense_cells`, `tui_redraw`, `cursor_motion`, `sixel`,
`hyperlinks`, `grapheme_clusters`, `scroll_region`, `plain_ascii`; after it, the same list. A
threshold is an absolute MiB/s floor (before + about 0.7 x the prototype's gain). Every guard
fixture stays >= 0.95 x its before value. A guard below that: re-run before and after
interleaved, best of 3. Still below, and the step's code is not on that fixture's path:
rebuild both with `RUSTFLAGS="-C llvm-args=-align-all-functions=6 -C
llvm-args=-align-all-nofallthru-blocks=5"` (measurement only, never committed); within 5 %
there is code placement, which acceptance.md records as such (evidence.md: S7 and
`tui_redraw`). Numbers go into `acceptance.md`.

## Steps

| Step | Design | Files (crates/vt/src unless noted) | Threshold after the step |
|---|---|---|---|
| S1 | text.md | `terminal/print.rs`, `terminal/terminal_tests.rs`, `tests/feed_chunking.rs` | `csi_mix` >= 105 |
| S2 | styles-graphemes.md | `grid/screen.rs` (`set_template`), `terminal/terminal_tests.rs` | `dense_cells` >= 205 |
| S3 | styles-graphemes.md | `intern.rs`, `intern_tests.rs` | `combining_unique` >= 21 |
| S4 | styles-graphemes.md | `intern.rs`, `intern_tests.rs`, `tests/feed_chunking.rs` | `combining_unique` >= 40 |
| S5 | parser.md | `parser/mod.rs`, `parser/ground.rs`, `parser/state.rs`, `parser/parser_tests.rs`, `tests/feed_chunking.rs` | `cursor_motion` >= 285, `tui_redraw` >= 355, `heavy_sgr` >= 280, `vim_session` >= 240, `csi_mix` >= 145; `long_osc` >= 0.95 x before (the `step` inlining rule) |
| S6 | text.md | `grid/row.rs`, `grid/row_tests.rs`, `grid/screen/print.rs`, `grid/screen/screen_tests.rs`, `terminal/print.rs`, `tests/feed_chunking.rs` | `cjk_wide` >= 310, `unicode_symbols` >= 310, `long_lines` >= 900 |
| S7 | scrolling.md | `grid/screen.rs`, `grid/row.rs`, `grid/screen/integrity.rs`, `grid/screen/screen_tests.rs`, `snapshot/snapshot_tests.rs` | `scroll_region` >= 475, `scroll_region_small` >= 31 |
| S8 | scrolling.md | `grid/screen.rs` (`scroll_up`), `terminal/terminal_tests.rs`, `docs/user/events.md`, `tests/feed_chunking.rs` | `scroll_region_small` >= 50 |
| S9 | - | `acceptance.md`, compare run | 21 goldens ok; `vt-bench grid --check` passes |

## User docs (in the step that changes the behaviour)

| File | Step | Change |
|---|---|---|
| `docs/user/events.md` | S8 | `RowsScrolled` / `RowsTrimmed` row: "a partial region at the top of the alternate screen moves its rows (`RowsScrolled`), nothing is trimmed" |

S1-S7 change no documented behaviour. `docs/user/api-map.md`: nothing (no new public item).
`docs/perf/README.md`: nothing (the next release records the numbers).

## Gate (after every step)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --all-features               # vt-paranoid widens the integrity walk
```

No `unsafe`, no new dependency, no new `pub` item: `scripts/public-api.sh check` (CI) must
not change. `RowRef`, `Span` and `GraphemeArena` gain private fields only.

## Commits (docs/agents/commits.md)

`docs(spec): second round of engine hot paths` first, then one per step, the subject naming
the fixture and the measured before -> after:

| Step | Subject (numbers from acceptance.md) |
|---|---|
| S1 | `perf(terminal): repeat REP copies through the print run, csi_mix A -> B MiB/s` |
| S2 | `perf(grid): reuse a background-only pen as its erase style, dense_cells A -> B MiB/s` |
| S3 | `perf(intern): size the grapheme sweep index once, combining_unique A -> B MiB/s` |
| S4 | `perf(intern): key graphemes by hash chain, no boxed key, combining_unique A -> B MiB/s` |
| S5 | `perf(parser): run CSI headers in a local loop, cursor_motion A -> B MiB/s` |
| S6 | `perf(grid): write glyph runs with edge-only pair release, cjk_wide A -> B MiB/s` |
| S7 | `perf(grid): restamp rotated rows once per batch, scroll_region A -> B MiB/s` |
| S8 | `perf(grid): scroll a partial alternate-screen top region in place, scroll_region_small A -> B MiB/s` |
| S9 | `chore(bench): compare run i7-12700-win11 <date>` (with `acceptance.md`) |

Bodies end with `Spec: docs/specs/0018-hot-paths-round-2`. S8's body lists the behaviour
change from scrolling.md.
