# 0003: Cell, intern, width (phase 2)

## Goal

What one grid position stores: an 8-byte packed `Cell`, `Style` with `Attrs` and colours, the
per-terminal `intern` tables a cell points into (styles, extras, grapheme clusters with a sweep,
OSC 8 hyperlinks), and the `scalar_width` / `cluster_width` column rules.

## Non-goals

- Grids, the print path, mode `?2027`, SGR parsing (phases 3, 6); nothing calls this code yet.
- Grid predicates, wide-pair repair, RIS `clear` of style/extras tables, extras reclamation
  (phases 3, 6, 10). `FeedStats` (phase 6) reads the counters defined here.

## Files in this spec

| File | Covers |
|---|---|
| [cell.md](cell.md) | `Cell` bit layout and methods, `CellContent`, `CellWidth`, `Semantic`, `Attrs`, colours, `Style` |
| [intern.md](intern.md) | caps table, ids, `Extras`, `InternTable`, `HyperlinkTable`, `Interner` |
| [graphemes.md](graphemes.md) | `GraphemeArena`, sweep trigger, `GraphemeRemap` |
| [width.md](width.md) | `scalar_width`, `cluster_width`, the tricky-case table |
| [tests.md](tests.md) | unit tests, proptests, exhaustive width test, user chapter outline |
| [files.md](files.md) | files, `lib.rs` re-exports, dependencies, rules, commit subject |

## Public API (names fixed by the roadmap)

Root: `Attrs, Cell, CellContent, CellWidth, Color, NamedColor, Rgb, Semantic, Style,
scalar_width, cluster_width`. `intern::*`: list in [files.md](files.md). Nothing else `pub`.

## Acceptance criteria

- [ ] Quality gate, `cargo test -p oneterm-vt --no-default-features`, and
      `RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps --all-features` pass
- [ ] `const _: () = assert!(size_of::<Cell>() == 8);` in `cell.rs`
- [ ] Every test in [tests.md](tests.md) passes, incl. proptests and `every_scalar_value`
- [ ] Every cap in [intern.md](intern.md) holds, degrades as stated, and is counted
- [ ] `docs/user/cells-and-styles.md` exists; its Rust blocks run under `cargo test --doc`
- [ ] No `unsafe`, no `unwrap`/`expect`/`panic!` in non-test code; no extra `pub` item
- [ ] Commit subject: `feat(cell): packed Cell, intern tables and Unicode widths` (minor)

## Decisions

1. `' '` and `'\0'` share content 0, so a blank has one encoding and derived `Eq` is exact.
2. Grapheme storage is UTF-8: `intern(&str)`, `resolve -> &str` (not `&[char]`).
3. Grapheme hard caps `1 << 18` entries / `1 << 20` scalars, below the 21-bit id space, to
   bound memory; the sweep trigger doubles after a large live set.
4. `HyperlinkTable` has a 4 MiB text cap besides the 65,536-link cap.
5. `cluster_width` delegates emoji/VS/ZWJ rules to `UnicodeWidthStr`, clamped to 2.
6. `scalar_width -> u8` (controls are 0), not `Option<u8>`.
7. `Cell::semantic()` and `Cell::is_protected()` are `pub` getters.
8. `NamedColor` has 29 variants (owner confirmed).
9. Overflow = grow-only counters, plus `None` from `HyperlinkTable::intern` (owner confirmed).
10. Keep `rustc-hash`; caps bound the worst probe chain (owner confirmed).
11. Pin `unicode-width = "=0.2.2"`: the width table is a compatibility contract (owner confirmed).
