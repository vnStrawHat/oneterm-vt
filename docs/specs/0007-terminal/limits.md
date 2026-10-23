# Limits: every ceiling, and what hostile input does

Rule (roadmap): no input panics, none grows memory without a bound, every drop is counted.
`docs/user/limits.md` (chapter 08) publishes this table; new constants are crate-private
unless listed `pub`.

## Ceilings

| Ceiling | Value (const) | Past it | Counter |
|---|---|---|---|
| CSI/DCS params incl. sub-params | 32 (`parser::MAX_PARAMS`) | sequence dispatched with `ignore`, not acted on | `unhandled_sequences` |
| intermediates | 2 (`parser::MAX_INTERMEDIATES`) | same | `unhandled_sequences` |
| param value | 65,535 (saturates) | clamped by every consumer (`CSI 99999 A` = to the margin) | - |
| OSC payload | 2 KiB (`OSC_INLINE`); 8 MiB (`OSC_LARGE`) for numbers routed non-`Drop` with `large` | tail dropped, `truncated = true`; OSC 7 and OSC 9;4 then reject | `truncated_osc` (+ `unhandled_sequences` for a reject) |
| OSC params | parser: 16 (`MAX_OSC_PARAMS`), the 16th holds the rest | the 16th is re-split on `;` (builtins that iterate, and every forwarded `Osc`): one span per parameter, so spans <= payload bytes + 1 (<= 2,049 inline; <= 8 Mi + 1 only for a `large(n)` number); `params` list shrinks to 4096 on the next `feed` | - |
| DCS / APC payload | 16 MiB (`DCS_MAX_BYTES`) | aborted, no answer | `aborted_dcs` (DCS) |
| DECRQSS / XTGETTCAP payload | 8 KiB (`QUERY_MAX_BYTES`) | answers nothing; buffer keeps <= 8 KiB capacity | `unhandled_sequences` (once) |
| XTGETTCAP names / name length | 16 (`XTGETTCAP_MAX_NAMES`) / 128 bytes (`XTGETTCAP_MAX_NAME_BYTES`) | name dropped, never truncated | `unhandled_sequences` (once per request) |
| title stack | 16 (`TITLE_STACK_MAX`) | oldest dropped | - |
| kitty flag stack | 8 (`KEYBOARD_STACK_MAX`) | oldest dropped; pop `>= len` empties | - |
| `?2027` cross-chunk carry | 32 scalars (`CLUSTER_CARRY_MAX`) | cluster not carried; continuation starts a new cluster | `dropped_cluster_carries` |
| product name | 64 bytes after dropping C0/DEL/C1 (`PRODUCT_NAME_MAX`) | cut at a char boundary; empty -> engine name | - |
| REP | 65,535 prints per sequence | bounded loop | - |
| event arena / lists | shrink to 1 MiB (`EVENT_ARENA_SOFT`) / 4096 entries (`EVENT_LIST_SOFT`) on the next `feed` | peak memory is linear in one feed's input, never retained | - |
| event arena offset | `u32::MAX` bytes | event dropped | `unhandled_sequences` |
| scrollback | 10,000 default, 1,000,000 max (`grid::DEFAULT_SCROLLBACK`, `grid::SCROLLBACK_MAX`) | oldest rows trimmed, `RowsTrimmed` | `rows_scrolled` counts entries |
| screen size | 1024 x 2048 (`Size::clamped`) | clamped by `new` and `resize` | - |
| styles / extras | 65,536 each (spec 0003) | default style / no link | `style_table_exhausted` / `hyperlink_table_exhausted` |
| hyperlinks | 65,536 links, 4 MiB text (spec 0003) | link dropped, text prints; RIS empties | `hyperlink_table_exhausted` |
| grapheme arena | 2^18 entries, 2^20 scalars; sweep from 65,536 / 2^18 (spec 0003) | cluster stored as blank; end-of-feed sweep reclaims dead ids | `grapheme_table_exhausted` |
| grapheme length | 16 scalars (spec 0003) | cut | `grapheme_truncated` |
| wide glyph in 1 column / last column without autowrap | - | dropped (spec 0004) | `wide_glyphs_dropped` |

## Hostile streams (one test each in `tests/feed_limits.rs`; capacities via the public `EventBatch` accessors)

| Stream | Bound that holds |
|---|---|
| 1 MiB random bytes (biased to `ESC [ ] P _ ; ? 0-9 BEL CAN`), 4 KiB chunks, 3 seeds | no panic; every feed's tier-3 walk passes (debug) |
| 16 MiB OSC 52 store with `large(52)` | payload cut at 8 MiB, `truncated_osc 1`; arena back to <= 1 MiB after the next feed |
| `ESC ] 7501;` + 8 MiB of `;` routed `Forward` + `large(7501)` | one `Osc` event with at most `OSC_LARGE + 1` params, no panic; `param_capacity() <= 4096` after the next feed |
| `ESC ] 7501;` + 8 MiB of `;`, `Forward` without `large` | payload cut at 2 KiB: one `Osc` with at most 2,049 params, `truncated_osc 1` |
| 100,000 `CSI 22 t` | `title_depth() == 16` |
| 100,000 `CSI > 1 u` then `CSI < 65535 u` | stack empty, `CSI ? u` -> `ESC [ ? 0 u` |
| `DCS + q` + 4000 `;` | 16 replies, `unhandled_sequences == 1` |
| `DCS $ q` + 9 KiB | no reply, `unhandled 1`, `aborted 0`, payload capacity <= 8 KiB |
| 70,000 distinct OSC 8 URIs, each printing one char | `hyperlink_table_exhausted > 0`, every char printed |
| 24x80, scrollback 100; 300,000 distinct clusters (`U+4E00 + i % 20000` + `U+0300 + i / 20000`), 64 KiB feeds | entries stay <= 2^18 and drop at least once (sweep); `grapheme_table_exhausted` total 0 |
| 100,000 `BEL` in one feed, then `a` | first batch 100,000 `Bell` + `Repaint`; after the second feed `event_capacity() <= 4096` |
| `CSI 65535 b` after `x`, 10 times | 655,350 prints, no panic, scrollback within its limit |
| `?2027` + `a` then 20,000 combining marks one per feed | `dropped_cluster_carries == 1` in total |
| Every reply query 10,000 times in one feed | replies in order; arena shrinks after the next feed |

What the embedder still owns: its own `scrollback_limit` choice, its OSC routes' payload
validation, and the policy for every event (clipboard, title, notification, cwd).
