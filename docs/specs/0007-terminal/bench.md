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
pub(crate) struct MemoryCost { content: &'static str, heap_bytes: u64, heap_bytes_per_row: f64 }
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

Machine, command (`cargo run -p oneterm-vt-bench --release -- all --mib 32`) and the three tables
are pasted here by the coder after 6e (informational; targets are phase 12's).
