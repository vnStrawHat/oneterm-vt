# Limits

Terminal output is untrusted. No input panics, none grows memory without a bound, and every drop is
counted in the `FeedStats` that `Terminal::feed` returns. Nothing here is an error: a sequence past
a ceiling is cut, ignored or degraded, and you read about it in a counter.

## Example: hostile input and its counters

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, Size, Terminal};

let mut term = Terminal::new(Size { rows: 24, cols: 80 }, Config::default());
let mut batch = EventBatch::new();

let mut stream = b"\x1b]0;".to_vec();
stream.extend(std::iter::repeat_n(b't', 3000)); // an OSC over its 2 KiB ceiling
stream.extend_from_slice(b"\x07\x1b[?9999z\x1b]52;c;!!!!\x07");
let stats = term.feed(&stream, &mut batch, Instant::now());

assert_eq!(stats.truncated_osc, 1); // cut at 2 KiB, still delivered
assert_eq!(stats.unhandled_sequences, 1); // CSI ? 9999 z is not implemented
assert_eq!(stats.malformed_sequences, 1); // OSC 52 payload is not base64
assert_eq!(term.title().map(str::len), Some(2047));

// Counters are per call, never cumulative.
let stats = term.feed(b"plain text", &mut batch, Instant::now());
assert_eq!(stats.unhandled_sequences, 0);
assert_eq!(stats.bytes, 10);
```

## Ceilings

| Ceiling | Value | Past it | Counter |
|---|---|---|---|
| CSI/DCS parameters (sub-parameters included) | 32 | the sequence is ignored | `unhandled_sequences` |
| intermediates | 2 | the sequence is ignored | `unhandled_sequences` |
| a parameter value | 65,535 | saturates; each consumer clamps | - |
| OSC payload | 2 KiB; 8 MiB for a number given `OscRoutes::large` and not `Drop` | tail dropped, `truncated` set; a truncated OSC 8 or OSC 7 is rejected, so no link opens | `truncated_osc` |
| OSC parameters | 16 from the parser, the last re-split on `;` | one span per `;` for builtins and forwarded OSCs; at most payload bytes + 1 spans, plus up to 15 for the separators the parser already split | - |
| DCS / APC payload | 16 MiB | aborted, no answer | `aborted_dcs` (DCS) |
| DECRQSS / XTGETTCAP payload | 8 KiB | no answer | `unhandled_sequences` (once) |
| XTGETTCAP names | 16 per request, 128 bytes each | the name is dropped, never cut | `unhandled_sequences` (once) |
| title stack / kitty flag stack | 16 / 8 | the oldest is dropped | - |
| `?2027` cluster carried across chunks | 32 scalars | not carried; the next chunk starts a new cluster | `dropped_cluster_carries` |
| cluster in a cell | 16 scalars | cut | `grapheme_truncated` |
| grapheme arena | 262,144 entries, 1,048,576 scalars; swept after each feed | stored as a blank | `grapheme_table_exhausted` |
| styles / extras | 65,536 each; an extras entry is freed once no cell names it (sweep every 4,096 new entries) | the default style / no link or image | `style_table_exhausted` / `hyperlink_table_exhausted` |
| image side | 4096 px per axis | pixels past it are dropped | - |
| image decode buffer | 64 MiB (4096 x 4096 RGBA) | cannot be exceeded | - |
| images waiting for `take_graphics` | 128 MiB of pixels (each image counts at least its struct size) | the oldest is dropped and released | `graphics_dropped` |
| image placements | 256 live | the oldest is released | - |
| hyperlinks | 65,536 links, 4 MiB of text | the link is dropped, the text prints | `hyperlink_table_exhausted` |
| REP | 65,535 prints per sequence | clamped | - |
| product name | 64 bytes after dropping control characters | cut at a character | - |
| scrollback | 10,000 default, 1,000,000 max | oldest rows trimmed (`RowsTrimmed`) | `rows_scrolled` counts entries |
| screen size | 1024 rows x 2048 columns | clamped | - |
| event arena and lists | shrink to 1 MiB / 4096 entries on the next `feed` | one feed's peak is never kept | - |

## What each counter means

The meaning of every field is part of the API; counting something else is a major change.

| Field | Counts, for this call |
|---|---|
| `bytes` | bytes fed |
| `rows_scrolled` | rows that left the top of a screen into its history (line feeds, wraps, IND, NEL, SU, DL, ED 2 on the primary); on the alternate screen they count although dropped at once |
| `unhandled_sequences` | sequences parsed but not acted on: unknown keys, values the engine does not implement, rejected OSCs, a shut DECRQCRA gate, payloads past their ceiling, dropped OSC numbers |
| `malformed_sequences` | OSC 52 stores that are not canonical base64 or not UTF-8 |
| `truncated_osc` | OSC strings that lost bytes to the size cap, also those you do not route |
| `aborted_dcs` | DCS strings ended by `CAN`/`SUB` or by the 16 MiB cap |
| `hyperlink_table_exhausted` | OSC 8 links dropped because a table was full |
| `style_table_exhausted` | SGR states that fell back to the default style |
| `grapheme_truncated` | clusters cut at 16 scalars |
| `grapheme_table_exhausted` | clusters stored as a blank because the arena was full |
| `dropped_cluster_carries` | `?2027` clusters over 32 scalars |
| `wide_glyphs_dropped` | wide glyphs that could not be placed (one column, or the last column with autowrap off) |
| `graphics_dropped` | images whose pixels were dropped before `take_graphics` (the queue passed 128 MiB) |

`FeedStats` is `#[non_exhaustive]` and read-only: a new counter is a minor version.

## What you still own

- Your `Config::scrollback_limit`: memory is about 8 bytes per cell of history.
- The payload validation of every OSC you forward, and the policy for every event: clipboard,
  title, notification, working directory, bell.
- Whether DECRQCRA may read the screen (`Config::allow_screen_readback`, off by default).
- Rate-limiting what you write back: a program can ask for thousands of `Reply` events per feed.
