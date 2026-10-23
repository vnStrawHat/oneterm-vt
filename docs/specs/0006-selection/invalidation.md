# Invalidation

Two mechanisms, no overlap:

- **Content that moves or dies** (scrolls, IL/DL, trim, wipe, resize) needs no code here: those
  primitives update or kill anchors, and a dead end makes `to_range` return `None`.
- **Content erased in place** keeps its row ids, so anchors cannot see it. The dispatcher asks
  `Selection::invalidated_by` **before** running the erase and clears the selection on `true`.

## `Invalidation`

```rust
#[non_exhaustive]                         // embedders construct it to ask; never need to match
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Invalidation { EraseScreen, EraseAbove, EraseBelow, EraseLine, EraseHistory, Reset, SwapAlt }
```

## `invalidated_by(&self, grid, inv) -> bool`

Let `top`, `bottom` = the two anchor rows ordered; `screen = grid.screen_of(top)` (the screen the
selection is on); `cursor = screen.cursor().pos.row`.

1. Either anchor dead -> `true` (for every variant).
2. `Reset`, `SwapAlt`, `EraseScreen` -> `true`.
3. Else, "does the selection meet the rows the op blanks":

| Variant | VT op | Blanks rows | `true` iff |
|---|---|---|---|
| `EraseLine` | EL 0/1/2 | the cursor row | `top <= cursor <= bottom` |
| `EraseBelow` | ED 0 | `cursor..=newest` | `bottom >= cursor` |
| `EraseAbove` | ED 1 | `screen_top..=cursor` | `top <= cursor && bottom >= screen_top` |
| `EraseScreen` | ED 2, DECALN, DECCOLM | the screen | always (rule 2) |
| `EraseHistory` | ED 3 | `oldest..screen_top` | `top < screen_top` |

- Row-level on purpose: `EL 0` right of a selection on the same row still clears it. Cheap, and
  it never leaves a selection over erased cells.
- `EraseScreen` clears every live selection, also one wholly in history and one whose content
  ED 2 on the primary moves into history with its ids (Decision 11): "clear" drops the selection.
- The rules read the selection's own screen, not the active one; a selection on the inactive
  screen is judged against that screen's frozen cursor (Decision 11).
- ECH, DCH, ICH, IRM and plain printing over selected cells do not invalidate: the selection
  keeps its cells and copies what they hold now.
- `EraseHistory` on the alternate screen is `false` for its own selections (no history). `trim`
  would kill history anchors anyway; the predicate lets the dispatcher release at the same point
  as the other erases.

## Row-moving operations (no call; anchors do it)

| Operation | Effect on a selection |
|---|---|
| full-screen scroll into history (LF at the bottom) | ids keep their content: unchanged range, same text |
| scrollback trim past an end's row, ED 3, RIS | that anchor dies -> `None` |
| region scroll (SU/SD/IL/DL/RI inside a region) | each end in the region moves with its row; an end on a row scrolled out of the region dies -> `None`; an end outside stays (Decision 15) |
| alternate-screen `wipe` | ends on its rows die |
| viewport scroll (`scroll_viewport`) | nothing: ranges are in row-id space |
| resize, rows only | ids kept; released rows below the cursor kill ends on them |
| resize, columns, primary | ends remap to their characters (spec 0005); trimmed rows kill them |
| resize, alternate screen | ends keep their row, col clamps to `cols - 1` |

## What phase 6 calls (recorded here; not this phase's code)

| Dispatch | Call before the op |
|---|---|
| EL 0/1/2 | `EraseLine` |
| ED 0 / 1 / 2 / 3 | `EraseBelow` / `EraseAbove` / `EraseScreen` / `EraseHistory` |
| DECALN (`ESC # 8`), DECCOLM | `EraseScreen` |
| `?47` / `?1047` / `?1049` set or reset that actually switches screens | `SwapAlt` |
| RIS | `Reset` |

- On `true`: `selection.take()` then `release`. The `Terminal` holds `Option<Selection>`.
- **Prune after every `feed` and `resize`**: phase 6 adds crate `Selection::is_dead(&self, grid)
  -> bool` (either anchor dead) and releases a dead selection. It must not prune on
  `to_range() == None`: a fresh click is an empty `Simple` selection and must survive output.
- Wrappers phase 6 adds on `Terminal`: `selection_start(pos, side, kind)` (clears first),
  `selection_update`, `selection_range`, `has_selection` (= `selection_range().is_some()`),
  `selection_text`, `selection_clear` (idempotent), `select_all`, `hit_test` (below),
  `row_text` (text.md). Each passes `&config.semantic_escape_chars` and `&interner.graphemes`.

**`Terminal::hit_test(viewport_row: f32, col: f32) -> (Pos, Side)`** (phase 6): negative and NaN
-> 0; row clamps to the viewport, col to `cols - 1`; `Pos.row = viewport.top + row`;
`Side::Left` iff `col.fract() < 0.5`, and a col past the last column is `Right`.

With ends in two lanes, `invalidated_by` answers from the primary screen; phase 6 prunes on dead ends, not lanes.
