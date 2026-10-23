# Tests

Unit tests sit in sibling files, wired as `#[cfg(test)] mod cell_tests;` etc. Each test checks
one behaviour. `proptest` is already a dev-dependency (phase 1).

## `cell_tests.rs`

| Test | Assert |
|---|---|
| `empty_cell_is_a_default_space` | `EMPTY.to_bits() == 0`; content `Scalar(' ')`, `Narrow`, `Semantic::None`, not protected, `StyleId::DEFAULT`, `ExtrasId::NONE`; `Cell::default() == EMPTY` |
| `space_and_nul_have_one_encoding` | `EMPTY.with_content(Scalar(' ')) == EMPTY`; `Scalar('\0')` reads back `' '` and equals `EMPTY` |
| `content_edges_round_trip` | `'\u{10FFFF}'`, `'a'`, `'\u{1}'`; `Grapheme(GraphemeId(0))`, `Grapheme(GraphemeId(GRAPHEME_ID_LIMIT - 1))` |
| `out_of_range_grapheme_id_falls_back` | `Grapheme(GraphemeId(GRAPHEME_ID_LIMIT))` and `u32::MAX` read back `Grapheme(GraphemeId(0))`; other fields unchanged |
| `bit_layout_is_the_documented_one` | one literal per field: `with_style(StyleId(0xABCD)).to_bits() == 0xABCD << 32`; extras `<< 48`; `Wide` `1 << 22`; `LeadingWideSpacer` `3 << 22`; `Output` `3 << 24`; protected `1 << 26`; `Grapheme(5)` `5 | 1 << 21`; `Scalar('A')` `0x41` |
| `spacer_roles` | `is_spacer` false for `Narrow`, `Wide`; true for both spacers |
| `text_char_reads_scalar_or_cluster_base` | scalar cell -> its char; grapheme `"e\u{301}"` -> `'e'`; unknown grapheme id -> `' '` |
| `default_style_is_foreground_on_background` | `Style::DEFAULT` fields; `Style::default() == Style::DEFAULT` |
| `all_underlines_is_the_five_underline_flags` | equals the union of the 5; contains no other flag |
| `style_stays_small` | `const _: () = assert!(size_of::<Style>() <= 16);` |

`mod props` (proptest, default case count):

- `scalar_cells_round_trip`: any `char`, `CellWidth`, `Semantic`, `bool`, `u16` style, `u16`
  extras -> every getter returns its input (`'\0'` reads `' '`); `to_bits() & RESERVED == 0`.
- `grapheme_cells_round_trip`: same with `Grapheme(GraphemeId(0..GRAPHEME_ID_LIMIT))`.
- `setters_touch_only_their_field`: random cell from random fields, then one random setter ->
  the five other getters are unchanged.

## `intern_tests.rs`

Helper `unique_cluster(n: u32, len: usize) -> String`: base `U+4E00 + n % 20_992`, then
`U+0300 + n / 20_992`, then `len - 2` copies of `U+0301` (one cluster of `len` scalars).
Entry-count tests use `len = 2`; the chars test uses `len = 16`.

| Test | Assert |
|---|---|
| `defaults_are_id_zero` | `style(Style::DEFAULT) == StyleId::DEFAULT`; `extras(Extras::NONE) == ExtrasId::NONE`; fresh `entries() == 1` |
| `equal_values_share_an_id` | two equal RGB styles -> one id, `entries() == 2` |
| `ids_are_stable` | intern 1,000 distinct styles, re-intern in reverse -> same ids; each resolves to its value |
| `unknown_ids_resolve_to_defaults` | `resolve_style(StyleId(999))`, `resolve_extras(ExtrasId(999))` -> defaults |
| `style_table_full_falls_back_and_counts` | 65,535 distinct styles -> `entries() == 65_536`; next new -> `StyleId::DEFAULT`, `exhausted() == 1`; a known style still returns its id, count unchanged |
| `one_extras_entry_per_image` | same `Extras { graphic: Some(GraphicId(7)), .. }` 10,000 times -> one id |
| `grapheme_dedupe_and_reserved_zero` | `"e\u{301}"` twice -> same id; `""` and `" "` -> id 0; `resolve(0) == " "`; unknown id -> `""` |
| `grapheme_truncates_at_16_scalars` | 16 scalars: kept, `truncated() == 0`; 17 scalars: first 16 kept, `truncated() == 1`; multi-byte scalars cut on a char boundary |
| `grapheme_full_falls_back_to_space` | `with_limits(4, 1 << 20)`: 3 new clusters fit, the 4th -> id 0, `exhausted() == 1`; same with `with_limits(1 << 18, 8)` and a scalar overflow |
| `sweep_triggers_on_entries` | 65,534 unique 2-scalar clusters -> false; one more -> true |
| `sweep_triggers_on_chars` | unique 16-scalar clusters: false while `chars() < 1 << 18`, true at the call that crosses it, with `entries()` far below 65,536 |
| `sweep_keeps_live_and_remaps` | 100 unique (ids 1..=100); live = ids `1, 4, ..., 100` + a duplicate + unknown `GraphemeId(5_000)` + id 0 -> `kept() == 34`; each `get(old)` resolves to the old text; dropped/unknown -> `GraphemeId(0)`; `entries() == 35` |
| `sweep_keeps_counters` | `truncated`/`exhausted` equal before and after |
| `sweep_trigger_doubles_after_a_large_live_set` | 70,000 unique -> `needs_sweep`; sweep all live -> false (trigger now 140,002 entries); 70,000 more -> still false; one more -> true |
| `hyperlink_identity` | explicit `(id, uri)` shared; same id + other uri differs; implicit by uri shared; implicit ids are `"1"`, `"2"`; explicit `"1"` never aliases implicit `"1"`; `Some("")` is implicit |
| `hyperlink_full_by_count` | 65,536 distinct short links fit; the next -> `None`, `exhausted() == 1`; an existing link still returns its id |
| `hyperlink_full_by_bytes` | explicit ids `format!("{n:04}")`, 2,048-byte URIs (2,052 bytes each): 2,044 fit, the 2,045th -> `None`, `exhausted() == 1` |
| `hyperlink_clear` | after `clear`: `is_empty`, next implicit id is `"1"`, `exhausted` kept |
| `interner_routes_to_its_tables` | each `Interner` method round-trips through its field's table |

## `width_tests.rs`

- `width_cases`: the table in [width.md](width.md) as `const CASES: &[(&str, Option<u8>, u8)]`
  (`None` where the S column is `-`). One loop, failure message names the row number.
- `zwj_family_per_scalar_sum_is_8`.
- `every_scalar_value` (the roadmap's "width table test over every scalar value"): for each
  `char` in `'\0'..=char::MAX`: `scalar_width <= 2`; `is_control()` -> 0; otherwise
  `cluster_width(c.encode_utf8(&mut buf)) == scalar_width(c)`. If the pinned crates disagree
  for some scalars, list them in a commented `EXCEPTIONS` slice and report them; do not change
  the functions.

If a `width_cases` row disagrees with the pinned crates (rows 15, 21 and 29 rest on the
crate's documented rules rather than a run), stop and report the row; do not edit it silently.

## Doctests: `docs/user/cells-and-styles.md` (chapter 15, coder writes, <= 120 lines)

1. Example first: `Interner::default()`; a `Style` with `fg: Color::Rgb(Rgb { r: 255, g: 0,
   b: 0 })` and `Attrs::BOLD | Attrs::UNDERLINE`; `style()` -> id; `Cell::EMPTY.with_content(
   CellContent::Scalar('x')).with_style(id)`; `resolve_style(cell.style_id())` equals it.
2. "What a cell is": 8 bytes, the fields, ids point into the terminal's `Interner`; a renderer
   resolves `style_id`/`extras_id`/grapheme ids, never stores them across a grapheme sweep.
3. Grapheme cell: `grapheme("e\u{301}")` + `resolve_grapheme` + `text_char`.
4. Width in 5 lines: 0 attaches to the previous cell, 1 narrow, 2 wide; ambiguous is 1; VS16
   on an emoji base is 2, VS15 is 1; with `?2027` a whole cluster is one cell. Asserts on
   `scalar_width('\u{6F22}') == 2` and `cluster_width("\u{1F44D}\u{1F3FD}") == 2`.
5. Limits table: the caps from [intern.md](intern.md) and what happens past each.

Wire: `#[cfg(doctest)] #[doc = include_str!("../../../docs/user/cells-and-styles.md")]
mod user_guide_cells_and_styles {}` in `lib.rs`.
