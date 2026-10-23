# Bench: tiers 2, 4, 5 in `vt-bench`

Numbers are informational until phase 12 (`grid --check` stays "not implemented yet").

## Tier 2 `grid` (`crates/vt-bench/src/throughput.rs`)

- `grid_pass(bytes) -> Duration`: `Terminal::new(Size { rows: LINES, cols: COLUMNS },
  Config::default())`, one `EventBatch`, `now = Instant::now()` (untimed); time only
  `for c in bytes.chunks(CHUNK) { term.feed(black_box(c), &mut batch, now); }`; then
  `black_box(&term)`. `feed` clears the batch itself.
- `measure_tier` gains a fixture filter: `measure_tier(mib, include: impl Fn(&Fixture) -> bool,
  pass)`. Tier 1 includes all ten; tier 2 every fixture except `sixel` (phase 10 adds it). Same
  cycles, warm-up, summary and table format as tier 1.
- Section header: `## Tier 2: parse + grid (Terminal::feed)`.

## Tier 4 `resize` (`crates/vt-bench/src/resize.rs`)

```rust
pub(crate) const RESIZE_DEPTHS: [u32; 3] = [0, 10_000, 100_000];
pub(crate) struct ResizeCost { scrollback_rows: u32, grow_us: f64, shrink_us: f64 } // perf_history
pub(crate) fn measure_resize() -> Vec<ResizeCost>;
```

Per depth: `Terminal::new(80x24, Config { scrollback_limit: depth, .. })`; feed (untimed, 64 KiB
chunks) the `long_lines` fixture generated at `(depth + 24) * 96` bytes, so history is full and
every line reflows. Then `RUNS` times: time `resize(100x40, BottomAnchor)` (grow), then
`resize(80x24, BottomAnchor)` (shrink). Report the median of each in microseconds. Geometry is
its own; never compared with tiers 1-3. Table: `| Scrollback rows | grow us | shrink us |`
(`{:.1}`), header `## Tier 4: resize 80x24 <-> 100x40`.

## Tier 5 `rss` (`crates/vt-bench/src/memory.rs`)

```rust
#![allow(unsafe_code, reason = "GlobalAlloc is an unsafe trait; this module only forwards to System")]
pub(crate) struct CountingAllocator;            // live bytes in a static AtomicUsize (Relaxed)
unsafe impl GlobalAlloc for CountingAllocator { alloc, dealloc, realloc }  // alloc_zeroed: default
pub(crate) fn live_heap_bytes() -> usize;
pub(crate) const MEMORY_ROWS: usize = 10_000;
pub(crate) struct MemoryCost { content: String, heap_bytes: u64, heap_bytes_per_row: f64 }
pub(crate) fn measure_memory() -> Vec<MemoryCost>;
```

- The **only** `unsafe` in the workspace until phase 11, lifted for this module alone by the inner
  `#![allow]` above (the workspace denies `unsafe_code`; `deny` is overridable, `forbid` is not).
  Each `unsafe` block carries a `// SAFETY:` comment: same layout in, `System`'s pointer out;
  counters are `fetch_add` / `fetch_sub` of `layout.size()` (realloc: add new, sub old, only when
  the result is non-null). No panics, no allocation inside the allocator.
- `main.rs`: `#[global_allocator] static ALLOCATOR: memory::CountingAllocator = ..;`. It applies
  to every tier; the cost is one relaxed atomic per allocation, off the measured hot paths.
- Kinds, one full row each, fed `MEMORY_ROWS` times in 64 KiB chunks into a fresh 160x45
  `Terminal` (`Config::default()`, 10,000 rows of scrollback):
  `plain` = 160 `.` + CRLF; `unicode` = 80 x `U+56FD` + CRLF; `styled` = per column `k`
  `ESC[38;2;{k%256};{3k%256};{7k%256}m` + `b'a' + k % 26`, then `ESC[0m` CRLF; `mixed` = per column
  `ESC[48;5;{k%256}m` before even `k`, char `' '` if `k % 8 == 0` else `b'a' + k % 26`, then `ESC[0m` CRLF.
- `heap_bytes` = `live_heap_bytes()` after the feeds minus the value just before
  `Terminal::new` (input bytes excluded; the terminal and its batch included);
  `heap_bytes_per_row = heap_bytes / MEMORY_ROWS`. Table `| Content | heap bytes | bytes/row |`,
  header `## Tier 5: live heap after 10000 scrollback rows`.

## CLI and record (`main.rs`, `perf_history.rs`)

| Invocation | Runs |
|---|---|
| `grid [--mib N] [--json --machine T] [--out F]` | tier 2 |
| `resize [...]`, `rss [...]` | tier 4 / tier 5 (`--mib` accepted, unused) |
| `all [...]` | tiers 1, 2, 4, 5 in that order, one report / one record |
| `grid --check ...`, `render` | unchanged error `'<cmd>' is not implemented yet` (phases 12, 7) |

`Tiers` gains, in schema order, `tier2_grid` (now also written), `tier4_resize:
Vec<ResizeCost>`, `tier5_memory: Vec<MemoryCost>`; each `#[serde(default, skip_serializing_if =
"Vec::is_empty")]` except `tier2_grid`, which stays required on read. Field names = docs/perf
schema (`scrollback_rows`, `grow_us`, `shrink_us`, `content`, `heap_bytes`, `heap_bytes_per_row`).

## Unit tests (`vt-bench`)

| Test | Assert |
|---|---|
| `tier2_skips_sixel` | the include filter yields 9 names, no `sixel`, table order kept |
| `grid_pass_feeds_every_byte` | 1 MiB `plain_ascii` -> a non-zero duration; a fresh terminal fed the same bytes has `lines_produced() > 0` |
| `resize_rows_have_three_depths` | `measure_resize` (depths overridden to `[0, 100]` via a private helper) -> two rows, finite non-negative values |
| `memory_kinds_are_full_rows` | each kind's row through a 160-col `Terminal` produces exactly one line (`lines_produced` +1) and no wrap |
| `counting_allocator_tracks_a_vec` | around a live `Vec::<u8>::with_capacity(64 << 20)`, `live_heap_bytes` is >= 48 MiB higher, and falls back by >= 48 MiB after drop (margins absorb parallel tests) |
| `record_has_tiers_2_4_5` | serialised record holds `tier2_grid`, `tier4_resize`, `tier5_memory` in schema order; a tier-1-only record still omits them |

## Acceptance notes

Recorded after 6e (informational; the targets are phase 12's). Machine: Intel Core i7-12700
(12C/20T), Windows 11 26300, rustc 1.96.0, release profile, nothing else building. Command:
`cargo run -p oneterm-vt-bench --release -- all --mib 32 --machine "<machine>" --out report.md`.
Geometry 160x45, 32 MiB per fixture, median of 5 cycles after 1 warm-up, 64 KiB chunks.

Tier 2 `grid` (parse + grid, `Terminal::feed`). "Before" is the first 6e run; "after" is the
re-run with the narrow-run print path, the SGR no-op early return and the cheaper erase-style
update (same machine, command `grid --mib 32`):

| Fixture | before MiB/s | before ns/B | after MiB/s | after ns/B | after spread |
|---|---:|---:|---:|---:|---:|
| `plain_ascii` | 82.9 | 11.50 | 158.1 | 6.03 | 10% |
| `long_lines` | 85.8 | 11.11 | 157.7 | 6.05 | 8% |
| `heavy_sgr` | 178.5 | 5.34 | 202.3 | 4.71 | 5% |
| `tui_redraw` | 126.0 | 7.57 | 179.8 | 5.31 | 1% |
| `scroll_region` | 61.8 | 15.44 | 91.8 | 10.38 | 1% |
| `cjk_wide` | 143.1 | 6.66 | 145.1 | 6.57 | 23% |
| `dense_cells` | 157.6 | 6.05 | 164.6 | 5.79 | 6% |
| `scrolling` | 86.3 | 11.05 | 165.0 | 5.78 | 29% |
| `osc_9_7` | 118.4 | 8.05 | 165.6 | 5.76 | 12% |

Tier 4 `resize` (`crates/vt-bench/src/resize.rs`)

```rust
pub(crate) const RESIZE_DEPTHS: [u32; 3] = [0, 10_000, 100_000];
pub(crate) struct ResizeCost { scrollback_rows: u32, grow_us: f64, shrink_us: f64 } // perf_history
pub(crate) fn measure_resize() -> Vec<ResizeCost>;
```

Per depth: `Terminal::new(80x24, Config { scrollback_limit: depth, .. })`; feed (untimed, 64 KiB
chunks) the `long_lines` fixture generated at `(depth + 24) * 96` bytes, so history is full and
every line reflows. Then `RUNS` times: time `resize(100x40, BottomAnchor)` (grow), then
`resize(80x24, BottomAnchor)` (shrink). Report the median of each in microseconds. Geometry is
its own; never compared with tiers 1-3. Table: `| Scrollback rows | grow us | shrink us |`
(`{:.1}`), header `## Tier 4: resize 80x24 <-> 100x40`.

## Tier 5 `rss` (`crates/vt-bench/src/memory.rs`)

```rust
#![allow(unsafe_code, reason = "GlobalAlloc is an unsafe trait; this module only forwards to System")]
pub(crate) struct CountingAllocator;            // live bytes in a static AtomicUsize (Relaxed)
unsafe impl GlobalAlloc for CountingAllocator { alloc, dealloc, realloc }  // alloc_zeroed: default
pub(crate) fn live_heap_bytes() -> usize;
pub(crate) const MEMORY_ROWS: usize = 10_000;
pub(crate) struct MemoryCost { content: String, heap_bytes: u64, heap_bytes_per_row: f64 }
pub(crate) fn measure_memory() -> Vec<MemoryCost>;
```

- The **only** `unsafe` in the workspace until phase 11, lifted for this module alone by the inner
  `#![allow]` above (the workspace denies `unsafe_code`; `deny` is overridable, `forbid` is not).
  Each `unsafe` block carries a `// SAFETY:` comment: same layout in, `System`'s pointer out;
  counters are `fetch_add` / `fetch_sub` of `layout.size()` (realloc: add new, sub old, only when
  the result is non-null). No panics, no allocation inside the allocator.
- `main.rs`: `#[global_allocator] static ALLOCATOR: memory::CountingAllocator = ..;`. It applies
  to every tier; the cost is one relaxed atomic per allocation, off the measured hot paths.
- Kinds, one full row each, fed `MEMORY_ROWS` times in 64 KiB chunks into a fresh 160x45
  `Terminal` (`Config::default()`, 10,000 rows of scrollback):
  `plain` = 160 `.` + CRLF; `unicode` = 80 x `U+56FD` + CRLF; `styled` = per column `k`
  `ESC[38;2;{k%256};{3k%256};{7k%256}m` + `b'a' + k % 26`, then `ESC[0m` CRLF; `mixed` = per column
  `ESC[48;5;{k%256}m` before even `k`, char `' '` if `k % 8 == 0` else `b'a' + k % 26`, then `ESC[0m` CRLF.
- `heap_bytes` = `live_heap_bytes()` after the feeds minus the value just before
  `Terminal::new` (input bytes excluded; the terminal and its batch included);
  `heap_bytes_per_row = heap_bytes / MEMORY_ROWS`. Table `| Content | heap bytes | bytes/row |`,
  header `## Tier 5: live heap after 10000 scrollback rows`.

## CLI and record (`main.rs`, `perf_history.rs`)

| Invocation | Runs |
|---|---|
| `grid [--mib N] [--json --machine T] [--out F]` | tier 2 |
| `resize [...]`, `rss [...]` | tier 4 / tier 5 (`--mib` accepted, unused) |
| `all [...]` | tiers 1, 2, 4, 5 in that order, one report / one record |
| `grid --check ...`, `render` | unchanged error `'<cmd>' is not implemented yet` (phases 12, 7) |

`Tiers` gains, in schema order, `tier2_grid` (now also written), `tier4_resize:
Vec<ResizeCost>`, `tier5_memory: Vec<MemoryCost>`; each `#[serde(default, skip_serializing_if =
"Vec::is_empty")]` except `tier2_grid`, which stays required on read. Field names = docs/perf
schema (`scrollback_rows`, `grow_us`, `shrink_us`, `content`, `heap_bytes`, `heap_bytes_per_row`).

## Unit tests (`vt-bench`)

| Test | Assert |
|---|---|
| `tier2_skips_sixel` | the include filter yields 9 names, no `sixel`, table order kept |
| `grid_pass_feeds_every_byte` | 1 MiB `plain_ascii` -> a non-zero duration; a fresh terminal fed the same bytes has `lines_produced() > 0` |
| `resize_rows_have_three_depths` | `measure_resize` (depths overridden to `[0, 100]` via a private helper) -> two rows, finite non-negative values |
| `memory_kinds_are_full_rows` | each kind's row through a 160-col `Terminal` produces exactly one line (`lines_produced` +1) and no wrap |
| `counting_allocator_tracks_a_vec` | around a live `Vec::<u8>::with_capacity(64 << 20)`, `live_heap_bytes` is >= 48 MiB higher, and falls back by >= 48 MiB after drop (margins absorb parallel tests) |
| `record_has_tiers_2_4_5` | serialised record holds `tier2_grid`, `tier4_resize`, `tier5_memory` in schema order; a tier-1-only record still omits them |

## Acceptance notes

Recorded after 6e (informational; the targets are phase 12's). Machine: Intel Core i7-12700
(12C/20T), Windows 11 26300, rustc 1.96.0, release profile, nothing else building. Command:
`cargo run -p oneterm-vt-bench --release -- all --mib 32 --machine "<machine>" --out report.md`.
Geometry 160x45, 32 MiB per fixture, median of 5 cycles after 1 warm-up, 64 KiB chunks.

Tier 2 `grid` (parse + grid, `Terminal::feed`):

| Fixture | MiB/s | ns/B | spread |
|---|---:|---:|---:|
| `plain_ascii` | 82.9 | 11.50 | 9% |
| `long_lines` | 85.8 | 11.11 | 10% |
| `heavy_sgr` | 178.5 | 5.34 | 11% |
| `tui_redraw` | 126.0 | 7.57 | 6% |
| `scroll_region` | 61.8 | 15.44 | 2% |
| `cjk_wide` | 143.1 | 6.66 | 5% |
| `dense_cells` | 157.6 | 6.05 | 6% |
| `scrolling` | 86.3 | 11.05 | 2% |
| `osc_9_7` | 118.4 | 8.05 | 8% |

Tier 4 `resize` 80x24 <-> 100x40:

| Scrollback rows | grow us | shrink us |
|---:|---:|---:|
| 0 | 3.4 | 2.3 |
| 10000 | 602.5 | 625.8 |
| 100000 | 10777.8 | 7786.2 |

Tier 5 `rss`, live heap after 10000 scrollback rows (the terminal plus its batch):

| Content | heap bytes | bytes/row |
|---|---:|---:|
| `plain` | 13639963 | 1364.0 |
| `unicode` | 13639963 | 1364.0 |
| `styled` | 13647817 | 1364.8 |
| `mixed` | 13643849 | 1364.4 |
