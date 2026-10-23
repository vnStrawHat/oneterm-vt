# `width.rs`: `scalar_width` and `cluster_width`

Module doc: two width functions; phase 6's print path picks one by mode `?2027` (grapheme
clustering). Reset (power-on): per scalar with `scalar_width`, as most terminals do. Set: the
run is split into clusters and each measured with `cluster_width`, so a whole cluster takes
one cell. Both are pure, total, never panic, and never return more than 2.

## Signatures

```rust
pub fn scalar_width(c: char) -> u8;        // 0, 1 or 2
pub fn cluster_width(text: &str) -> u8;    // width of the FIRST extended grapheme cluster of `text`
```

`u8` rather than `CellWidth`: 0 is a real answer (attach to the previous cell) and `CellWidth`
names a cell's role in a pair, not a measurement. Not `Option`: controls never reach the print
path (the parser executes C0, DEL and C1), so "control" folds into 0.

## `scalar_width` rules

`UnicodeWidthChar::width(c).unwrap_or(0).min(2) as u8` (`unicode-width` 0.2, non-CJK table):

| Input | Width |
|---|---|
| control (`Cc`: C0, DEL, C1), incl. NUL | 0 (`width` is `None`) |
| combining / `Grapheme_Extend`, default-ignorable (ZWJ, VS15, VS16), Hangul V/T jamo | 0 |
| East Asian Wide or Fullwidth, incl. Emoji_Presentation scalars | 2 |
| East Asian Ambiguous | 1 (non-CJK context) |
| everything else, incl. unassigned and private use | 1 |
| the few scalars `unicode-width` rates 3 (Khmer sign beyyal) | clamped to 2 |

## `cluster_width` rules, in order

1. `let Some(cluster) = text.graphemes(true).next()` (`unicode-segmentation`, extended
   clusters); none (empty text) -> 0.
2. First scalar of the cluster is a control (`char::is_control`) -> 0 (consistent with
   `scalar_width`; `"\r\n"` is one cluster and must not measure 1).
3. Otherwise `UnicodeWidthStr::width(cluster).min(2) as u8`.

Step 3 delegates the emoji rules to `unicode-width` 0.2's string rules: emoji ZWJ sequences,
modifier (skin tone) sequences and emoji presentation sequences (`base + VS16`) are 2; a text
presentation sequence (`VS15`) on an Emoji_Presentation base is 1; a lone ZWJ / VS / combining
mark is 0. VS16 widens only bases with the Emoji property (`"a\u{FE0F}"` stays 1). Clamping
to 2 covers what the string rules sum (unqualified ZWJ sequences, regional-indicator pairs).

No hand-written emoji tables: the dependency already carries them. A debug check that `text`
holds one cluster is not needed: measuring only the first cluster is the contract.

## Tricky cases (also the `width_cases` test table)

`S` = `scalar_width` of the single scalar, `C` = `cluster_width` of the text.

| # | Text | What | S | C |
|---|---|---|---|---|
| 1 | `"a"` | ASCII | 1 | 1 |
| 2 | `"\u{6F22}"` | CJK ideograph | 2 | 2 |
| 3 | `"\u{FF21}"` | fullwidth A | 2 | 2 |
| 4 | `"\u{3000}"` | ideographic space | 2 | 2 |
| 5 | `"\u{B1}"` | ambiguous `±` | 1 | 1 |
| 6 | `"\u{E000}"` | private use | 1 | 1 |
| 7 | `"\u{0}"`, `"\u{7}"`, `"\u{1B}"`, `"\u{7F}"`, `"\u{9B}"` | controls | 0 | 0 |
| 8 | `"\r\n"` | CRLF cluster | - | 0 |
| 9 | `"\u{301}"` | combining acute alone | 0 | 0 |
| 10 | `"e\u{301}"` | e + combining acute | - | 1 |
| 11 | `"\u{200D}"` | ZWJ alone | 0 | 0 |
| 12 | `"\u{FE0F}"` | VS16 alone | 0 | 0 |
| 13 | `"\u{FE0E}"` | VS15 alone | 0 | 0 |
| 14 | `"\u{2764}"`, `"\u{2764}\u{FE0F}"` | heart: text default, VS16 | 1 | 1, 2 |
| 15 | `"\u{231A}"`, `"\u{231A}\u{FE0E}"` | watch: emoji default, VS15 | 2 | 2, 1 |
| 16 | `"1\u{FE0F}"`, `"#\u{FE0F}\u{20E3}"` | digit VS16, keycap | - | 2, 2 |
| 17 | `"a\u{FE0F}"` | VS16 on a non-emoji base | - | 1 |
| 18 | `"\u{6F22}\u{FE0E}"` | VS15 does not narrow a CJK ideograph | - | 2 |
| 19 | `"\u{1F642}"` | emoji presentation scalar | 2 | 2 |
| 20 | `"\u{1F44D}\u{1F3FD}"` | thumbs up + skin tone | - | 2 |
| 21 | `"\u{1F3FD}"` | skin tone modifier alone | 2 | 2 |
| 22 | `"\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}"` | ZWJ family | - | 2 |
| 23 | `"\u{1F468}\u{200D}\u{1F4BB}"` | man technologist | - | 2 |
| 24 | `"\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}"` | rainbow flag | - | 2 |
| 25 | `"\u{1F1FA}\u{1F1F8}"` | flag (two regional indicators) | - | 2 |
| 26 | `"\u{1100}"`, `"\u{1161}"`, `"\u{11A8}"` | Hangul L, V, T jamo | 2, 0, 0 | 2, 0, 0 |
| 27 | `"\u{1100}\u{1161}\u{11A8}"` | L+V+T, one cluster | - | 2 |
| 28 | `"\u{AC00}"` | precomposed Hangul syllable | 2 | 2 |
| 29 | `"\u{17D8}"` | Khmer beyyal (`unicode-width` says 3) | 2 | 2 |
| 30 | `"ab"`, `""` | first cluster only; empty | - | 1, 0 |

Also pinned: the per-scalar sum over row 22 is 8 (four wide emoji, three zero ZWJ), which is
what a terminal with `?2027` reset shows.

## Version pinning

The table is the compatibility contract: an `unicode-width` or `unicode-segmentation` upgrade that
changes a row fails `width_cases` and is a reviewed change (`fix`/`feat` commit naming it).
`unicode-width` is pinned `=0.2.2` in the root `Cargo.toml` (README decision 11).
