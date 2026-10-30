# Steps, files, acceptance, gate, commits

Order: gain per complexity; steps touch disjoint functions, so each is green on its own. The
prototype measured them in this order (evidence.md). S6 is the owner's decision (README):
land it last and only if approved; dropping it touches nothing else.

## How to measure (every threshold below)

```bash
cargo build -p oneterm-vt-bench --release
exe=$(cygpath -w "$CARGO_TARGET_DIR/release/vt-bench.exe")
cmd //c start "" //b //wait //high //affinity 10 "$exe" grid --fixture NAME --mib 32
```

Pinned to logical CPU 4, high priority, median of 5. Before a step, record the step's
fixtures plus the guard list `heavy_sgr`, `dense_cells`, `tui_redraw`, `cursor_motion`,
`csi_mix`, `plain_ascii`, `scrolling`, `cjk_wide`, `grapheme_clusters`, `scroll_region`;
after it, the same list. A threshold is an absolute MiB/s floor (before + about 0.7 x the
prototype's gain). Every guard stays >= 0.95 x its before value. A guard below that: re-run
before and after interleaved, best of 3. Still below, and the step's code is not on that
fixture's path: rebuild both with `RUSTFLAGS="-C llvm-args=-align-all-functions=6 -C
llvm-args=-align-all-nofallthru-blocks=5"` (measurement only, never committed); within 5 %
there is code placement, which acceptance.md records as such (0018 evidence.md: S7 and
`tui_redraw`). Known case: S2 and `scroll_region_small` stayed at 0.93 even aligned in the
prototype while its profile was equal (evidence.md); a drop of that size there, after S2
only, is recorded, not chased. Numbers go into `acceptance.md`.

## Steps

| Step | Design | Files (crates/vt/src unless noted) | Threshold after the step |
|---|---|---|---|
| S1 | parser.md | `parser/mod.rs`, `parser/ground.rs`, `parser/state.rs`, `parser/osc.rs`, `parser/parser_tests.rs`, `tests/feed_chunking.rs` | `long_osc` >= 7,000, `hyperlinks` >= 680, `osc_9_7` >= 270; `cursor_motion`, `tui_redraw` >= 0.95 x before (the per-byte loop condition) |
| S2 | osc-text.md | `terminal/osc.rs`, `intern.rs`, `intern_tests.rs`, `events/batch.rs`, `events/event_tests.rs`, `terminal/osc_tests.rs` | `hyperlinks` >= 1,100, `osc_9_7` >= 325 |
| S3 | sixel.md | `graphics/sixel.rs`, `graphics/sixel_tests.rs`, `tests/feed_chunking.rs` | `sixel_large` >= 37, `sixel` >= 51 |
| S4 | sixel.md | `grid/screen.rs` (`collect_*_ids`), `grid/terminal_grid_tests.rs` | `sixel` >= 58 |
| S5 | scrolling.md | `grid/screen.rs` (`linefeed`), `grid/anchor.rs`, `grid/anchor_tests.rs`, `grid/screen/screen_tests.rs`, `terminal/terminal_tests.rs` | `short_lines` >= 63, `osc_9_7` >= 350, `sixel` >= 69 |
| S6 (owner) | scrolling.md | `grid/row.rs`, `grid/screen.rs`, `grid/screen/reflow.rs`, `grid/terminal_grid.rs`; tests: `grid/row_tests.rs`, `grid/screen/screen_tests.rs`, `grid/screen/reflow_tests.rs`, `grid/terminal_grid_tests.rs`, `grid/resize_tests.rs`; `docs/user/grid.md` | `scroll_region_small` >= 63, `scroll_region` >= 595; `scrolling` >= 0.93 x before (the approved cost) |
| S7 | - | `acceptance.md`, compare run | 21 goldens ok; `vt-bench grid --check` passes |

## User docs (in the step that changes them)

| File | Step | Change |
|---|---|---|
| `docs/user/grid.md` ("Scrollback and memory") | S6 | "slots of 48 bytes" -> "slots of 8 bytes; a written row adds 48 bytes plus its cells" (the `heap_bytes() < 1_000_000` example still holds) |

No step changes documented behaviour, an event, a counter or a limit.
`docs/user/limits.md` stays as it is (S3 keeps the 64 MiB decode ceiling, now also the
peak). `docs/user/api-map.md`: nothing (no new public item). `docs/perf/README.md`: nothing
(the next release records the numbers).

## Gate (after every step)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --all-features               # vt-paranoid widens the integrity walk
```

No `unsafe`, no new dependency, no new `pub` item: `scripts/public-api.sh check` (CI) must
not change. `HyperlinkTable` and `Anchors` gain private fields only.

## Commits (docs/agents/commits.md)

`docs(spec): third round of engine hot paths` first, then one per step, the subject naming
the fixture and the measured before -> after:

| Step | Subject (numbers from acceptance.md) |
|---|---|
| S1 | `perf(parser): take OSC and DCS payloads in runs, long_osc A -> B MiB/s` |
| S2 | `perf(terminal): decode OSC text once, hyperlinks A -> B MiB/s` |
| S3 | `perf(graphics): hand the sixel decode buffer over as the image, sixel_large A -> B MiB/s` |
| S4 | `perf(grid): sweep style and extras ids over the used prefix, sixel A -> B MiB/s` |
| S5 | `perf(grid): line feed into history without the region scroll, short_lines A -> B MiB/s` |
| S6 | `perf(grid): box ring rows so rotations move 8 bytes, scroll_region A -> B MiB/s` (owner approved) |
| S7 | `chore(bench): cross-engine results after the third hot-path round` (with `acceptance.md`) |

Bodies end with `Spec: docs/specs/0019-hot-paths-round-3`. S6's body states the trade-off
(README, owner decision) with the measured numbers.
