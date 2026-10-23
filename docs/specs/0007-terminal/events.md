# Events: `events.rs`, `events/batch.rs`

An event is a value in a caller-owned batch, never a callback: no embedder code runs inside
`feed`, so the caller may hold its lock across `feed` and drain afterwards.

## `EventBatch` and spans (`events/batch.rs`)

```rust
#[derive(Debug, Default)]
pub struct EventBatch { events: Vec<VtEvent>, arena: Vec<u8>, params: Vec<ByteSpan>, repaint: bool }
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)] pub struct StrSpan { start: u32, len: u32 }
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)] pub struct ByteSpan { start: u32, len: u32 }
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)] pub struct ParamSpans { first: u32, count: u32 }
```

| Method | Vis | Rule |
|---|---|---|
| `new()` | pub | empty (= `Default`) |
| `events() -> &[VtEvent]`, `iter()`, `len()`, `is_empty()` | pub | byte order |
| `str(StrSpan) -> &str` | pub | out of range -> `""` (never panics) |
| `bytes(ByteSpan) -> &[u8]` | pub | out of range -> `&[]` |
| `params(ParamSpans) -> impl Iterator<Item = &[u8]>` | pub | out of range -> empty |
| `clear()` | pub | top of every `feed` (callers need not call it): empty all, `repaint = false`, then the shrink rule |
| `push(VtEvent)` | pub | payload-free events, or spans this batch owns; `Repaint` goes through `push_repaint` |
| `push_repaint()` | pub | idempotent within a batch |
| `push_title(&[u8]) -> bool` | pub | `Title(span)`; not UTF-8 -> `false`, nothing pushed |
| `push_clipboard_store(ClipboardKind, &[u8]) -> bool` | pub | `ClipboardStore`; not UTF-8 -> `false`, nothing pushed |
| `push_reply(&[u8])` | pub | `Reply(ByteSpan)` |
| `push_osc(code: u32, params: &[&[u8]], StringTerm, truncated: bool)` | pub | `Osc`, one span per element |
| `arena_capacity()`, `event_capacity()`, `param_capacity()` | pub | capacities, for allocation tests without an allocator hook |
| `push_text(&[u8], impl FnOnce(StrSpan) -> VtEvent) -> bool` | crate | generic form of `push_title` |
| `push_osc_params(code, &OscParams, StringTerm, truncated)` | crate | the engine's forward path: every param, and the 16th re-split on `;`, so each `;`-separated parameter is one span; iterates, no `Vec<&[u8]>` |
| `push_scroll_report(&ScrollReport)` | crate | `RowsScrolled` if `scrolled`, then `RowsTrimmed` if `trimmed` |
| `mark()`, `extend(&[u8])`, `finish_trimmed(mark) -> Option<StrSpan>`, `finish_lossy(mark) -> StrSpan` | crate | build one payload in place; `finish_trimmed` trims Unicode whitespace, on invalid UTF-8 rewinds to `mark` and gives `None`; `finish_lossy` repairs invalid UTF-8 (allocates only then) |
| `decode_base64(&[u8]) -> Option<StrSpan>` | crate | decode into the arena; rewinds, `None`, unless canonical base64 (osc.md) of UTF-8 |

The public `push_*` let an embedder or its tests build a batch by hand; the engine uses the
same methods.

- **Shrink on `clear`**: arena capacity > `EVENT_ARENA_SOFT` (1 MiB) -> `shrink_to(1 MiB)`;
  `events` / `params` capacity > `EVENT_LIST_SOFT` (4096) -> `shrink_to(4096)`. One hostile feed
  cannot pin its peak for the session; a steady stream reaches a high-water mark and then
  allocates nothing.
- Offsets are `u32`: a push that would take the arena past `u32::MAX` bytes is dropped and counted
  in `unhandled_sequences` (unreachable below a 4 GiB feed).
- A span is an index, not a borrow: kept past the next `feed` it reads other bytes (still valid
  UTF-8 for `str`), never UB. `VtEvent` is not `Clone`, so an event cannot outlive its batch
  without the borrow checker noticing.

## `VtEvent` (`events.rs`)

`#[non_exhaustive] #[derive(PartialEq, Eq, Debug)] pub enum VtEvent` (no `Clone`). Byte order;
`Repaint` last. Under `BuiltinAndForward` the typed event comes first, then `Osc`.

| Variant | Pushed when |
|---|---|
| `Repaint` | at most once, last, iff the feed made any `Dispatch` call (a hint, not damage) |
| `Bell` | `BEL` (0x07) outside a string |
| `Title(StrSpan)` | OSC 0/2 with non-empty trimmed text; `CSI 23 t` popping a title |
| `TitleReset` | OSC 0/2 whose trimmed text is empty; `CSI 23 t` popping "no title" |
| `IconName(StrSpan)` | OSC 1 (trimmed, may be empty) |
| `Reply(ByteSpan)` | every answer in queries.md; write verbatim to the program, in order |
| `ColorQuery { key: ColorKey, terminator: StringTerm }` | OSC 4/10/11/12/17/19 value `?` (colors.md) |
| `ClipboardStore { selection: ClipboardKind, text: StrSpan }` | OSC 52 store, decoded and UTF-8 |
| `ClipboardLoad { selection: ClipboardKind }` | OSC 52 payload `?` |
| `ScreenCleared` | ED 2, ED 3 (even with empty history), RIS; pushed before the op's scroll events |
| `RowsScrolled(RowsScrolled)` | any scroll report with `scrolled` (region moves, wraps included) |
| `RowsTrimmed { oldest: RowId }` | any scroll report with `trimmed` |
| `Cwd { host: StrSpan, path: StrSpan }` | OSC 7 (osc.md) |
| `Notification { title: StrSpan, body: StrSpan }` | OSC 9 other than `9;4`; `title` is empty for OSC 9 (kept for title-carrying forms) |
| `Progress(Progress)` | OSC 9;4 |
| `Pointer(StrSpan)` | OSC 22 |
| `ShellMark(ShellMark)` | OSC 133 A/B/C/D |
| `CursorStyleChanged` | `Terminal::cursor_style()` differs after a sequence (modes.md) |
| `Osc { code: u32, params: ParamSpans, terminator: StringTerm, truncated: bool }` | routed `Forward` or `BuiltinAndForward`; one span per `;`-separated parameter (the parser's 16th re-split), param 0 is the code text |

Phase 10 adds `GraphicReleased(GraphicId)` (non-exhaustive: minor).

**Minor-version notes** (contract changes vs the study material, recorded deliberately):
`ClipboardKind` and `FeedStats` are `#[non_exhaustive]` (new selections, new counters are minor
bumps; `FeedStats` is read-only for embedders, built only via `Default`).

Payload types (all `Clone, Copy, PartialEq, Eq, Hash, Debug`):

```rust
#[non_exhaustive] pub enum ClipboardKind { Clipboard, Primary, Secondary }      // c, p, s (minor note above)
#[non_exhaustive] pub enum Progress { Remove, Set(u8), Error(u8), Indeterminate, Paused(u8) }
#[non_exhaustive] pub enum ShellMark { PromptStart, PromptEnd, OutputStart,
                                       OutputEnd { exit_code: Option<i32> } }
pub use crate::parser::StringTerm;   // root re-export; the same type as the parser's
```

## `FeedStats` (`events.rs`)

`#[non_exhaustive] #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]`. Every counter
is **per `feed` call** (never cumulative), saturating. `Terminal::stats()` returns the last one.
The meaning of each field is API (docs/user/versioning.md).

| Field | Counts |
|---|---|
| `bytes: usize` | `bytes.len()` of this call |
| `rows_scrolled: u32` | rows that left the top of a screen into its history (sum of `ScrollReport::history_rows`): LF, wraps, IND, NEL, SU, DL at row 0, ED 2 on the primary; on the alternate screen (no history) they count although dropped at once |
| `unhandled_sequences: u32` | sequences parsed but not acted on; rules in dispatch-esc-c0.md |
| `malformed_sequences: u32` | routed but unusable payload: OSC 52 store not canonical base64 or not UTF-8 |
| `truncated_osc: u32` | OSCs dispatched with `truncated = true` (counted before routing, dropped ones too) |
| `aborted_dcs: u32` | DCS ended by CAN/SUB or by `DCS_MAX_BYTES` (any DCS kind) |
| `hyperlink_table_exhausted: u32` | OSC 8 links dropped as a table was full: delta of `hyperlinks.exhausted()` + `extras.exhausted()` |
| `style_table_exhausted: u32` | delta of `styles.exhausted()`: SGR states that fell back to the default style |
| `grapheme_truncated: u32` | delta of `graphemes.truncated()`: clusters cut at 16 scalars |
| `grapheme_table_exhausted: u32` | delta of `graphemes.exhausted()`: clusters stored as a blank (arena full) |
| `dropped_cluster_carries: u32` | `?2027` clusters over 32 scalars, once per placed cluster (dispatch-esc-c0.md) |
| `wide_glyphs_dropped: u32` | delta of both screens' `dropped_wide()`: wide glyphs that could not be placed |

Deltas: `feed` reads the interner and grid counters before `advance` and adds the saturating
difference after it (and after the grapheme sweep, which carries counters over).
