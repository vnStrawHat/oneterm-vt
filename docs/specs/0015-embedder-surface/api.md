# API: what is added, changed and kept

## A1. Mutable grid and intern access (`terminal.rs`)

```rust
impl Terminal {
    /// Both screens, mutably: write rows, move the viewport, open a batch.
    pub fn grid_mut(&mut self) -> &mut TerminalGrid;
    /// The intern tables, mutably: mint the ids a hand-written cell carries.
    pub fn interner_mut(&mut self) -> &mut Interner;
}
```

Placed next to `grid()` / `interner()`. Not feature-gated, not `#[doc(hidden)]`. Rustdoc of
`grid_mut` states, in this order:

1. Writes here bypass the parser: the selection, image placements, `ColorOverrides`, the title and
   the generation counter are not told. Fixtures, replay tools and viewport control are the uses.
2. `Screen::row_mut` stamps the row with the current batch. A `SnapshotState` that already saw that
   batch skips it: call `grid_mut().begin_batch()` before writing so the next update copies the row.
3. Moving the viewport by hand is `screen_mut().scroll_viewport(d)` (or `scroll_to_bottom()`)
   followed by `sync_anchors()`, so a resize before the next `feed` keeps the new viewport top.
   [`Terminal::scroll_viewport`] does both.
4. Nothing panics: `row_mut` returns `None` outside the live range, counts clamp.

Rustdoc of `interner_mut`:

1. Ids minted here stay valid while a cell in the grid carries them: `feed` sweeps graphemes and
   extras no row uses (a live grapheme id may be renumbered, its cells rewritten). Mint and
   write in one step, before the next `feed`.
2. A full table answers with its fallback id (`StyleId::DEFAULT`, `ExtrasId::NONE`, grapheme id
   0) and counts it in the table's own `exhausted()` / `truncated()`, not in `FeedStats`.

`Terminal::scroll_viewport` rustdoc gains one sentence: "Equivalent to
`grid_mut().screen_mut().scroll_viewport(delta)` followed by `grid_mut().sync_anchors()`."

## A2. Root re-exports (`lib.rs`)

```rust
pub use intern::{Extras, ExtrasId, GraphicId, Hyperlink, HyperlinkId, Interner};
```

Rule: an intern type gets a root name when an embedder keeps it in its own types or builds it
by hand. `GraphicId` is the payload of `VtEvent::GraphicReleased` and keys image caches;
`HyperlinkId` (from `SnapshotCell::hyperlink`) and `Hyperlink` key link caches; `ExtrasId` and
`Extras` are what a hand-written cell carries; `Interner` is what `Terminal::interner` returns.
`StyleId`, `GraphemeId` and the table types are passed straight back to `Interner` and stay
under `intern::` only. The `intern::` paths stay; both paths name the same item. The crate doc's
module list gains one line saying the intern ids above are also at the root.

## A3. `ColorKey` is exhaustive (`terminal/color.rs`)

Remove `#[non_exhaustive]` from `ColorKey`. Doc line: "Exhaustive on purpose: an embedder answers
a colour query from its theme and must name a default for every key; a new key is a major
change." No new variant: the slots `259..=268` stay unused and `from_index` keeps returning `None`
for them.

## A4. `parser::Dispatch` passes intermediates as a slice (`parser/`, `terminal/`)

```rust
pub trait Dispatch {
    fn esc(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
    fn csi(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
    fn dcs_hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
    // every other method unchanged
}
```

- The parser passes `self.intermediates.as_slice()`. Sinks match `(intermediates, byte)` with
  slice patterns (`([b'$'], b'q')`), as the terminal's own dispatcher already does.
- `parser::Intermediates` becomes `pub(crate)` and leaves the `pub use` list; `MAX_INTERMEDIATES`
  stays public (it is a documented limit). Doc guarantees on each method are unchanged.
- `dcs_hook` keeps `ignore`: a sink that streams a DCS payload must know the header overflowed.

## Kept as is (the embedder adapts; see gaps.md)

| Item | Kept behaviour | Why |
|---|---|---|
| `Screen::row_mut` | returns `Option<RowMut>`, `None` outside `oldest..=newest` | a stale id must not overwrite the live row sharing its ring slot |
| `Interner::style`, `Interner::extras` | take `Style` / `Extras` by value | both are `Copy`; nothing to borrow |
| `GraphemeArena::intern(&str)`, `resolve -> &str`, `Interner::resolve_grapheme -> &str` | clusters stored as UTF-8 | the copy, search and snapshot paths are all `str`; a `[char]` view would cost 4x memory |
| `SearchMatch::end_row` | a match may span soft-wrapped rows | `docs/user/search.md` "What a match means" already shows the per-row paint rule |
| `CSI 14 t` before `set_cell_pixels` | answers with `VIRTUAL_CELL` (`4;480;800` at 24x80) | 0011 D4: the reply matches the footprint images get |
