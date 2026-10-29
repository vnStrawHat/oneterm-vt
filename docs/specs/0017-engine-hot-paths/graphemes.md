# Items 2 and 7: graphemes (S7, S8)

## S7 root cause: `combining_unique` 1.1-1.6 MiB/s

1. **A sweep after every feed.** Every cell holds a unique cluster, so the live set (10,045 rows
   x 160 cells) is far above the arena cap (262,144). After a sweep that keeps >= cap/2,
   `sweep_trigger` returns the cap itself, the arena stays full and `needs_sweep()` is true at
   the end of every feed: a 46 ms sweep (walk 16,384 slots, re-intern 262,144 clusters with a
   boxed key each, `FxHashMap` remap of every cell) per 64 KiB. 83 % of the time. This is the
   case the `ponytail` note on `sweep_trigger` anticipated.
2. **Three interns per cell.** The scalar path attaches each mark alone: `a+U+300`,
   `a+U+300+U+301`, then the full cluster, each with a fresh `String` and a boxed key; two of
   the three entries are garbage at once and fill the arena 3x faster.
3. **Loss.** The sweep keeps the first 262,144 ids in slot order (not the newest rows) and maps
   the rest to id 0, and a full arena interns to id 0: the base letter becomes a blank.

## S7 design

| Item | Change |
|---|---|
| `GraphemeArena::sweep` (pub, signature unchanged) | keeps clusters in `live` order; from the first new cluster that would pass `max_entries / 2` entries or `max_chars / 2` scalars on, every further live id is dropped. With `kept <= cap / 2`, `sweep_trigger = max(base, 2 x kept) <= cap`, so at least `kept` new entries arrive between sweeps (amortised O(1) again). Delete the `ponytail` note. The fresh index is pre-sized to `2 x kept` |
| `GraphemeRemap` (pub type, same pub methods) | a dense `Vec<u32>` indexed by old id: the new id, or `DROPPED_BIT \| first scalar` (bit 31; ids < 2^18 and scalars < 2^21 never reach it). `get(old)` keeps its meaning (dropped -> `GraphemeId(0)`); an id not in `live` maps to `GraphemeId(0)` as today; `kept()` returns a stored count |
| `GraphemeRemap::content(old) -> CellContent` | new, `pub(crate)`: `Grapheme(new)` or `Scalar(first scalar)` for a dropped id |
| `RowMut::remap_graphemes` | writes `remap.content(old)`: a dropped cluster keeps its base glyph and width role |
| `TerminalGrid::sweep_graphemes` | passes live ids newest first: the active screen's rows from `newest` down to `oldest`, then the other screen's (`Screen::grapheme_ids` walks ids through `row(id)`, not slots) |
| `GraphemeArena::intern_joined(base: CellContent, tail: &str) -> GraphemeId` | new, `pub(crate)`: builds base + tail in an arena-owned scratch `String` (kept for its capacity), then `intern`; no allocation per call |
| `Screen::attach_marks(marks: &str, interner)` | replaces `attach(c)`: same target cell rules (pending wrap, spacer -> its wide cell), one `intern_joined`; when the arena refuses (id 0 for a non-empty key) the cell is left as it was (the marks are dropped, counted) |
| `print_glyph` width 0 | calls `attach_marks` with the scalar encoded into a 4-byte buffer |
| `print_scalars` | at a scalar that is width 0 and not a control, take the maximal run of such scalars and call `grid.attach_marks(run)` once; then `try_run = true` |
| `TerminalGrid::attach_marks` | new `pub(crate)` forwarder to the active screen |

Truncation at 16 scalars still happens in `intern` on the joined key, so the stored cluster is
the same as with one attach per mark.

## S8 root cause: `grapheme_clusters` (`?2027`) 27 MiB/s

- Every cluster, each ASCII letter included, goes through `place_cluster`; `cluster_width`
  segments the cluster a second time (`graphemes(true).next()`); each extra scalar is a separate
  `print` -> `attach` with its own `String` and an intern of a prefix; `carry_text` is rewritten
  per cluster. Profile: `attach` 3.3 M calls, 33 %.

## S8 design

- `print_clusters`, after the carried cluster: loop over the rest. While it starts with >= 2
  printable ASCII bytes and no single shift is pending, print all but the last of them
  through `grid.print_run` (S2's ASCII path), clear `carry`, continue. Otherwise take one
  cluster (`graphemes(true).next()`), `take_set()`, `place_cluster`. Two printable ASCII
  scalars always have a cluster boundary between them (UAX #29: no rule joins them), so only
  the last ASCII byte, which the next scalar or chunk may extend, needs the cluster path.
- `place_cluster`: width 0 -> `attach_marks(cluster)`; else print the first scalar (mapped)
  with the cluster width, then `attach_marks(rest)` once when `rest` is not empty.
  `preceding_char` = the cluster's last scalar (as now). Carry bookkeeping unchanged.
- `width::cluster_str_width(cluster: &str) -> u8` (`pub(crate)`): the width of one known
  cluster (control first -> 0, else `UnicodeWidthStr::width` clamped to 2). `cluster_width`
  calls it after segmenting; `place_cluster` calls it directly.

## Expected (pinned MiB/s, prototype)

| Fixture | HEAD | S7 | S8 |
|---|---:|---:|---:|
| `combining_unique` | 1.1-1.6 | 17.4-18.4 | 18.4 |
| `grapheme_clusters` | 24-27 | (shares the scratch) | 55-59 |

Left for later (README): one boxed key per unique cluster plus a ~19 ms sweep per ~131,000 new
clusters; removing the key needs a hash table with borrowed lookups, which std lacks.

## Behaviour changes (documented, not API)

- Live clusters beyond half of either cap: the oldest rows' clusters become their first scalar
  at the next sweep (before: arbitrary ones became blanks and the arena stayed full).
- A cluster refused by a full arena leaves the cell's glyph (before: a blank).
  `grapheme_table_exhausted` counts the same refusals; limits.md "stored as a blank" ->
  "kept without the new marks".
- `grapheme_truncated`: one per cut attach. A run of N > 15 marks in one print now counts 1
  (before N - 15); split across feeds it may count more. limits.md wording unchanged
  ("clusters cut at 16 scalars").
- Chunk invariance holds while the live clusters fit in half the arena (before: the whole
  arena); past that, which rows degrade depends on where feeds end, as before.
