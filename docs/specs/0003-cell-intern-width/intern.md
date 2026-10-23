# `intern.rs`: the tables a `Cell` points into

Module doc: one `Interner` per terminal, shared by the main and alternate screens (phase 6), so
an id never changes meaning on a screen swap. Two overflow strategies:

- **Styles, extras, hyperlinks never move.** Reuse, insert, or fall back. An issued id resolves
  to the same value for the table's life (until `HyperlinkTable::clear` on RIS).
- **Graphemes are collected** (`sweep` + remap, [graphemes.md](graphemes.md)), because unique
  clusters are the one attacker-reachable growth with no natural reuse.

**Overflow is never an error and never a panic.** Every table keeps saturating `u32` counters
(`exhausted()`, `truncated()`); they only grow, survive `sweep`/`clear`, and phase 6 maps their
per-`feed` delta to `FeedStats`. No `log` calls in this phase (dependency not added).

## Caps

| Table | Cap (const) | At the cap |
|---|---|---|
| `InternTable<T>` | 65,536 entries incl. id 0 (ids `0..=u16::MAX`), private `TABLE_CAPACITY` | new value -> id 0; `exhausted += 1`; known values still hit |
| `GraphemeArena` cluster length | 16 scalars, private `GRAPHEME_MAX_LEN` | cut at 16 scalars; `truncated += 1` |
| `GraphemeArena` entries | `1 << 18` incl. id 0, crate `GRAPHEME_ID_LIMIT` | new cluster -> `GraphemeId(0)` (`" "`); `exhausted += 1` |
| `GraphemeArena` scalars | `1 << 20`, private `GRAPHEME_MAX_CHARS` | same as entries |
| sweep due (entries) | `pub const GRAPHEME_SWEEP_ENTRIES: usize = 65_536` | `needs_sweep()` true |
| sweep due (scalars) | `pub const GRAPHEME_SWEEP_CHARS: usize = 1 << 18` | `needs_sweep()` true |
| `HyperlinkTable` links | 65,536, private `HYPERLINK_CAPACITY` | `intern` -> `None`; `exhausted += 1` |
| `HyperlinkTable` text | 4 MiB of `id` + `uri` bytes, private `HYPERLINK_MAX_BYTES` | same as links |

Worst-case heap (estimates): styles ~3 MiB, extras ~4 MiB, graphemes ~20 MiB (4 MiB text,
stored twice, plus spans and map), hyperlinks ~15 MiB. The byte cap exists as one URI can be 2 KiB
(`OSC_INLINE`); 65,536 of them would be 256 MiB.

## Ids (`pub` tuple field; derive `Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug`)

```rust
pub struct StyleId(pub u16);     // + Default; StyleId::DEFAULT = StyleId(0) -> Style::DEFAULT
pub struct ExtrasId(pub u16);    // + Default; ExtrasId::NONE = ExtrasId(0) -> Extras::NONE
pub struct GraphemeId(pub u32);  // + Default; GraphemeId(0) -> " "
pub struct HyperlinkId(pub u32); // index into HyperlinkTable, from 0
pub struct GraphicId(pub u64);   // owned by phase 10 graphics; only stored here
```

## `Extras`

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Extras { pub hyperlink: Option<HyperlinkId>, pub graphic: Option<GraphicId> }
impl Extras { pub const NONE: Extras = Extras { hyperlink: None, graphic: None }; }
```

`graphic` says which image covers the cell, not where: one entry per image, not per cell.

## `InternTable<T>` (hash-consing; `T: Copy + Eq + Hash + Default`)

```rust
pub struct InternTable<T> { entries: Vec<T>, index: FxHashMap<T, u16>, exhausted: u32 }
impl<T: Copy + Eq + Hash + Default> Default for InternTable<T>  // entries = [T::default()] at id 0
pub fn intern(&mut self, value: T) -> u16;  // hit -> its id; else push; full -> 0 + count
pub fn resolve(&self, id: u16) -> &T;       // unknown id -> &entries[0] (always present)
pub fn entries(&self) -> usize;             // incl. id 0
pub fn exhausted(&self) -> u32;
```

- `rustc_hash::FxHashMap`. Fx is not collision-resistant; caps bound the worst probe chain.
- Derive `Debug`. No `clear`/free/sweep this phase: `clear` lands with RIS (phase 6), extras
  reclamation with graphics (phase 10). Until then the extras table only grows, within its cap.

## `HyperlinkTable` and `Hyperlink`

```rust
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Hyperlink { pub id: Box<str>, pub uri: Box<str>, pub implicit: bool }
pub struct HyperlinkTable { links: Vec<Hyperlink>, explicit: FxHashMap<(Box<str>, Box<str>), u32>,
    implicit: FxHashMap<Box<str>, u32>, next_implicit: u32, bytes: usize, exhausted: u32 }
pub fn intern(&mut self, id: Option<&str>, uri: &str) -> Option<HyperlinkId>;
pub fn resolve(&self, id: HyperlinkId) -> Option<&Hyperlink>;   // unknown -> None
pub fn len(&self) -> usize;  pub fn is_empty(&self) -> bool;  pub fn exhausted(&self) -> u32;
pub fn clear(&mut self);  // RIS: drop all links, next implicit id back to 1; keeps `exhausted`
```

- Identity: explicit = `(id, uri)`; implicit (`None` or `Some("")`) = `uri` alone, so a TUI
  redrawing one link every frame holds one entry. The maps are separate: explicit `id=1` never
  aliases implicit link `1`.
- New implicit link: `next_implicit += 1`, `id` = its decimal text (first is `"1"`), `implicit
  = true`. A new link adds `id.len() + uri.len()` to `bytes` (checked against the byte cap).
- `None` means "drop the link attribute"; the text still prints. Derives `Debug, Default`.

## `Interner`

```rust
#[derive(Debug, Default)]
pub struct Interner { pub styles: InternTable<Style>, pub extras: InternTable<Extras>,
                      pub graphemes: GraphemeArena, pub hyperlinks: HyperlinkTable }
pub fn style(&mut self, style: Style) -> StyleId;            // full -> StyleId::DEFAULT
pub fn resolve_style(&self, id: StyleId) -> &Style;          // unknown -> Style::DEFAULT
pub fn extras(&mut self, extras: Extras) -> ExtrasId;        // full -> ExtrasId::NONE
pub fn resolve_extras(&self, id: ExtrasId) -> &Extras;       // unknown -> Extras::NONE
pub fn grapheme(&mut self, cluster: &str) -> GraphemeId;
pub fn resolve_grapheme(&self, id: GraphemeId) -> &str;
pub fn needs_grapheme_sweep(&self) -> bool;
```

Counters phase 6 maps to `FeedStats`: `styles.exhausted()`, `extras.exhausted()`,
`graphemes.exhausted()`, `graphemes.truncated()`, `hyperlinks.exhausted()`.
