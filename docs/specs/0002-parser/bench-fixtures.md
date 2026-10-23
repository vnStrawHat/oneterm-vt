# Bench fixtures and tier 1

## Fixtures (`crates/vt-bench/src/fixtures.rs`)

```rust
pub(crate) const COLUMNS: usize = 160;
pub(crate) const LINES: usize = 45;
pub(crate) struct Fixture { pub(crate) name: &'static str, pub(crate) about: &'static str,
                            pub(crate) make: fn(target: usize) -> Vec<u8> }
pub(crate) const FIXTURES: [Fixture; 10] = [/* the order of the table below */];
```

`make(target)` = optional prefix, then whole units `0, 1, 2, ...` appended until
`len >= target`. No RNG, no files, no partial unit. So `make(1)` is exactly prefix + unit 0,
which the golden tests use. `i` is the unit index (`usize`); `ESC` = `\x1b`, `BEL` = `\x07`.

| Name | About | Prefix | Unit `i` |
|---|---|---|---|
| `plain_ascii` | plain ASCII log lines | - | `the quick brown fox jumps over the lazy dog 0123456789 -- log line\r\n` (68 B) |
| `long_lines` | lines 5x the width, each wraps | - | 800 bytes `b'a' + k % 26` (`k` in `0..800`), then `\r\n` |
| `heavy_sgr` | a 24-bit SGR per char | - | `ESC[38;2;{i%256};{(i/3)%256};{(i/7)%256}m` + `b'a' + i%26`; then `\r\n` if `(i+1) % 160 == 0` |
| `tui_redraw` | CUP + short text | - | `ESC[{i%45+1};{(i*7)%160+1}H` + `stat: 42` |
| `scroll_region` | DECSTBM, then line feeds | `ESC[2;44r` | `tail output line ` + 30 `-` + `\r\n` (49 B) |
| `cjk_wide` | wide CJK fills every line | - | 80 x `U+56FD` (`E5 9B BD`) + `\r\n` (242 B) |
| `dense_cells` | 256-colour SGR per cell | - | one frame: `ESC[H`, then for `row` in `0..45`, `col` in `0..160`: `ESC[48;5;{(row*160+col)%256}m` + `b'a' + (row+col)%26` (no CR/LF; autowrap) |
| `scrolling` | grid-width plain lines | - | 160 x `.` + `\r\n` (162 B) |
| `sixel` | small Sixel images | - | `SIXEL_UNIT` below (70 B; 12x12 px: a red, then a green 6-row band) |
| `osc_9_7` | an OSC 9;7 status before every line | - | `OSC_UNIT` below (56 B) |

Fixed units, byte for byte:

```rust
const SIXEL_UNIT: &[u8] = b"\x1bPq\"1;1;12;12#0;2;100;0;0#1;2;0;100;0#0~~~~~~~~~~~~-#1~~~~~~~~~~~~\x1b\\\r\n";
const OSC_UNIT: &[u8] = b"\x1b]9;7;state=running;title=build\x07building the workspace\r\n";
```

Numbers in the table are written with `COLUMNS`/`LINES` in code (`160` = `COLUMNS`,
`45`/`44` = `LINES`/`LINES - 1`, `800` = `5 * COLUMNS`, `80` = `COLUMNS / 2`).

## Tier 1: `vt-bench parser` (`crates/vt-bench/src/throughput.rs`)

```rust
pub(crate) const RUNS: usize = 5;            // measured cycles; cycle 0 is a discarded warm-up
pub(crate) const CHUNK: usize = 64 * 1024;   // bytes per advance, like a PTY read
pub(crate) fn measure_tier(mib: usize, pass: impl FnMut(&[u8]) -> Duration) -> Vec<Throughput>;
pub(crate) fn parser_pass(bytes: &[u8]) -> Duration;
fn summarize(fixture: &str, len: usize, samples: &mut [Duration]) -> Throughput;
struct NullDispatch;                         // implements only print_str/execute, as no-ops
```

- `measure_tier`: for cycle `0..=RUNS`, for each fixture in order: `bytes = make(mib << 20)`
  (untimed, regenerated per cycle so 10 fixtures are never resident at once), `d = pass(&bytes)`,
  keep `d` if `cycle > 0`. Interleaving spreads machine drift across all fixtures.
- `parser_pass`: new `Parser` + `NullDispatch`; time only the loop
  `for c in bytes.chunks(CHUNK) { parser.advance(&mut sink, black_box(c)) }`; then
  `black_box(&parser)` so the work cannot be optimized away.
- `summarize`: sort; median = `samples[len / 2]`; `rate(d) = len / 2^20 / secs(max(d, 1 ns))`;
  `mib_per_second = rate(median)`, `ns_per_byte = median_ns / len`,
  `spread_percent = (rate(fastest) - rate(slowest)) / rate(median) * 100`.
- `Throughput` is the existing `perf_history::Throughput`, extended with `ns_per_byte` (files.md).

## CLI (phase 1 part of the scaffold's `USAGE`)

| Invocation | Behaviour |
|---|---|
| `parser [--mib N] [--json] [--machine T] [--out FILE]` | tier 1; `--mib` default 32, range `1..=4096` |
| `all ...` | every implemented tier; in phase 1 that is tier 1 only |
| `fixtures --out DIR [--mib N]` | writes `DIR/<name>.vt` (`make(mib << 20)`), creates `DIR`; one stdout line per file: `wrote DIR/<name>.vt (<bytes> bytes): <about>` |
| `--json` without `--machine` | error `--json needs --machine` (the record requires it), exit 1 |
| `--out FILE` on `parser`/`all` | report goes to `FILE` (parent dir created), stdout gets `wrote FILE` |
| `fixtures` without `--out` | error naming `--out`, exit 1 |

`--frames` stays accepted and unused until tier 3. `grid`, `render`, `resize`, `rss` stay stubs.

Table output (exact layout; numbers `{:.1}`, `{:.2}`, `{:.0}%`):

```text
# vt-bench: oneterm-vt 0.1.0

Geometry 160x45, 32 MiB per fixture, median of 5 cycles after 1 warm-up, 64 KiB chunks.
Machine: <TEXT>                      <- only with --machine

## Tier 1: parser alone (null Dispatch)

| Fixture | MiB/s | ns/B | spread |
|---|---:|---:|---:|
| `plain_ascii` | 1234.5 | 0.77 | 3% |
```

JSON output: one `PerfRecord` (schema in docs/perf/README.md) pretty-printed, keys in schema
order: `engine` `"oneterm-vt"`, `version` from `CARGO_PKG_VERSION`, `date` UTC `YYYY-MM-DD` of the
run, `machine`, `columns` 160, `lines` 45, `mib_per_fixture`, `runs` 5, `tiers` with
`tier1_parser` only (empty tiers are omitted). No `note` field.

## Acceptance notes

Machine: Intel Core i7-12700 (12C/20T), Windows 11 26300, rustc 1.96.0, release profile.
Command: `cargo run -p oneterm-vt-bench --release -- parser --mib 32`. One run of the shipped
build (`memchr(ESC)` ground scan):

| Fixture | MiB/s | ns/B | spread |
|---|---:|---:|---:|
| `plain_ascii` | 527.0 | 1.81 | 32% |
| `long_lines` | 718.4 | 1.33 | 58% |
| `heavy_sgr` | 157.9 | 6.04 | 38% |
| `tui_redraw` | 189.1 | 5.04 | 62% |
| `scroll_region` | 483.5 | 1.97 | 44% |
| `cjk_wide` | 438.7 | 2.17 | 50% |
| `dense_cells` | 150.4 | 6.34 | 18% |
| `scrolling` | 565.6 | 1.69 | 31% |
| `sixel` | 255.6 | 3.73 | 43% |
| `osc_9_7` | 287.8 | 3.31 | 23% |

Spreads of 20-60% mean the machine was noisy (desktop, other processes); only differences larger
than that are real. Expect to re-record on a quiet machine.

### Ground scan: `memchr(ESC)` vs `memchr3(ESC, LF, CR)` (D5)

Both variants were built and run alternately, three `parser --mib 32` runs each; MiB/s, median of
the three runs per fixture:

| Fixture | `memchr(ESC)` | `memchr3(ESC, LF, CR)` |
|---|---:|---:|
| `plain_ascii` | 449.5 | 355.7 |
| `long_lines` | 503.6 | 518.7 |
| `heavy_sgr` | 142.0 | 126.9 |
| `tui_redraw` | 179.7 | 165.4 |
| `scroll_region` | 453.2 | 325.6 |
| `cjk_wide` | 393.3 | 401.9 |
| `dense_cells` | 125.2 | 120.4 |
| `scrolling` | 488.0 | 439.7 |
| `sixel` | 250.5 | 241.3 |
| `osc_9_7` | 240.1 | 208.4 |

Verdict: `memchr(ESC)` is faster on 8 of 10 fixtures (`plain_ascii` +26%, `scroll_region` +39%) and
level within noise on the other two; D5 stands and `memchr3` was dropped.
