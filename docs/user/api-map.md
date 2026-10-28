# API map

Every public type and trait of `oneterm-vt`, grouped by what you do with it. Paths are
below `oneterm_vt::`. A row may fold the small types around one idea (a type and its id, an enum
and its payload). The last row of a group lists its functions and constants.

## Terminal and configuration

| Item | What it is for | Chapter |
|---|---|---|
| `Terminal` | the engine: `feed` bytes, `resize`, `snapshot_update`, `encode_key`; `grid_mut` and `interner_mut` for hand-written cells | [quickstart](quickstart.md) |
| `Config` | how a `Terminal` is built: scrollback, product name, OSC routes | [quickstart](quickstart.md) |
| `OscRoute`, `OscRoutes` | per OSC number: handle it, forward it to you, or drop it | [events](events.md) |
| `Mode`, `ModeState` | a terminal mode, and how a program set it (DECRQM answers) | [conformance](conformance.md) |
| `CursorShape`, `CursorStyle` | the cursor shape and blink, as a program set it | [snapshot](snapshot.md) |
| `KeyboardFlags` | kitty keyboard flags the program has pushed | [input](input.md) |
| `ColorKey`, `ColorOverrides` | a colour slot a program queries; the slots it changed | [events](events.md) |

## Events

| Item | What it is for | Chapter |
|---|---|---|
| `EventBatch` | caller-owned buffer `feed` fills; resolves every span | [events](events.md) |
| `VtEvent` | what happened: reply, title, bell, clipboard, OSC, repaint | [events](events.md) |
| `FeedStats` | per-feed counters: bytes, drops, truncations | [limits](limits.md) |
| `ByteSpan`, `StrSpan`, `ParamSpans` | indexes into their batch; read before the next `feed` | [events](events.md) |
| `ClipboardKind`, `Progress`, `ShellMark` | payloads of clipboard, progress and prompt-mark events | [events](events.md) |
| `StringTerm` (also `parser::StringTerm`) | how an OSC ended: `BEL` or `ST`; echo it in replies | [events](events.md) |

## Snapshot and drawing

| Item | What it is for | Chapter |
|---|---|---|
| `SnapshotState` | the reused copy of the screen you draw from | [snapshot](snapshot.md) |
| `SnapshotUpdate` | what an update did: `Full`, `Partial { scrolled }`, `Unchanged` | [snapshot](snapshot.md) |
| `SnapshotRow`, `SnapshotCell` | one copied row and its cells with style runs | [snapshot](snapshot.md) |
| `SnapshotContent`, `SnapshotCursor` | what a cell shows; where the cursor is | [snapshot](snapshot.md) |
| `StyleRun` | a style shared by a span of cells in a row | [snapshot](snapshot.md) |
| `Palette` | theme colours merged with the program overrides | [snapshot](snapshot.md) |
| `ModeSnapshot` | modes a renderer or input layer reads | [snapshot](snapshot.md) |
| `MouseProtocol`, `MouseEncoding`, `MouseReporting` | which mouse events the program wants, and how to write them | [input](input.md) |
| `SyncState`, `Watermark` | synchronized output (`?2026`) deadline; rows changed since your last copy | [threading](threading.md) |

## Cells and styles

| Item | What it is for | Chapter |
|---|---|---|
| `Cell` | one packed grid position | [cells-and-styles](cells-and-styles.md) |
| `CellContent`, `CellWidth` | scalar or cluster; narrow, wide head or spacer | [cells-and-styles](cells-and-styles.md) |
| `Style`, `Attrs` | colours and attribute bits of a cell | [cells-and-styles](cells-and-styles.md) |
| `Color`, `NamedColor`, `Rgb` | default, indexed or true colour; named slots | [cells-and-styles](cells-and-styles.md) |
| `Semantic` | shell-integration zone a cell was written in | [cells-and-styles](cells-and-styles.md) |
| functions | `scalar_width`, `cluster_width`: column width of a scalar or cluster | [cells-and-styles](cells-and-styles.md) |

## Positions, rows and resize

| Item | What it is for | Chapter |
|---|---|---|
| `Pos`, `grid::Pos` | a row and column | [grid](grid.md) |
| `Size`, `grid::Size` | rows and columns of a screen | [resize](resize.md) |
| `Viewport`, `grid::Viewport` | the rows currently visible | [grid](grid.md) |
| `RowId`, `grid::RowId` | stable name of a row content; key your caches on it | [grid](grid.md) |
| `RowRef`, `grid::RowRef` | read access to a row by id | [grid](grid.md) |
| `SeqNo`, `grid::SeqNo` | change stamp of a batch of input | [snapshot](snapshot.md) |
| `ResizePolicy`, `ResizeOutcome` | how a resize treats history; what it did | [resize](resize.md) |
| `grid::RowsScrolled` | rows whose ids moved, as reported by an event | [events](events.md) |

## Selection

| Item | What it is for | Chapter |
|---|---|---|
| `Selection` | a selection that follows content through scroll and reflow | [selection](selection.md) |
| `SelectionKind`, `Side` | simple, block, word or line; which half of a cell | [selection](selection.md) |
| `SelectionRange` | the cells a selection covers right now | [selection](selection.md) |
| `Invalidation` | what an erase or resize is about to do to selections | [selection](selection.md) |

## Graphics

| Item | What it is for | Chapter |
|---|---|---|
| `GraphicData` | decoded Sixel pixels, taken once with `take_graphics` | [graphics](graphics.md) |
| `Placement`, `SnapshotPlacement` | where an image sits on the grid, and as a renderer sees it | [graphics](graphics.md) |
| `VIRTUAL_CELL` | cell pixel size assumed until you call `set_cell_pixels` | [graphics](graphics.md) |

## `parser`

| Item | What it is for | Chapter |
|---|---|---|
| `parser::Parser` | byte-stream parser, usable without a grid | [parser](parser.md) |
| `parser::Dispatch` | the sink that receives printed text, controls and sequences | [parser](parser.md) |
| `parser::Params`, `parser::ParamGroups`, `parser::ParamSep` | CSI parameters, their groups and sub-parameter separators | [parser](parser.md) |
| `parser::OscParams` | the parameters of one OSC | [parser](parser.md) |
| constants | `MAX_PARAMS`, `MAX_INTERMEDIATES`, `MAX_OSC_PARAMS`, `OSC_INLINE`, `OSC_LARGE`, `DCS_MAX_BYTES` | [limits](limits.md) |

## `intern`

| Item | What it is for | Chapter |
|---|---|---|
| `intern::Interner` (also `Interner`) | the tables a cell ids point into | [cells-and-styles](cells-and-styles.md) |
| `intern::InternTable` | a capped, deduplicating table | [cells-and-styles](cells-and-styles.md) |
| `intern::StyleId`, `intern::ExtrasId` (also `ExtrasId`), `intern::Extras` (also `Extras`) | a cell style id and its link or image extras | [cells-and-styles](cells-and-styles.md) |
| `intern::GraphemeArena`, `intern::GraphemeId`, `intern::GraphemeRemap` | storage and ids of multi-scalar clusters | [cells-and-styles](cells-and-styles.md) |
| `intern::Hyperlink` (also `Hyperlink`), `intern::HyperlinkId` (also `HyperlinkId`), `intern::HyperlinkTable` | OSC 8 links | [cells-and-styles](cells-and-styles.md) |
| `intern::GraphicId` (also `GraphicId`) | identity of an image, named by `GraphicReleased` | [graphics](graphics.md) |
| constants | `GRAPHEME_SWEEP_CHARS`, `GRAPHEME_SWEEP_ENTRIES` | [cells-and-styles](cells-and-styles.md) |

## `grid`

| Item | What it is for | Chapter |
|---|---|---|
| `grid::TerminalGrid` | primary and alternate screen, anchors, resize | [grid](grid.md) |
| `grid::Screen` | one screen with scrollback; read rows through it | [grid](grid.md) |
| `grid::ScreenKind` | primary or alternate | [grid](grid.md) |
| `grid::Row`, `grid::RowMut`, `grid::RowHeader`, `grid::RowFlags` | a row of cells, its header and flag bits | [grid](grid.md) |
| `grid::Cursor`, `grid::CursorOrigin`, `grid::Charset`, `grid::PrintMode` | cursor and pen state; origin mode; charsets; print modes | [grid](grid.md) |
| `grid::Anchor`, `grid::AnchorId`, `grid::AnchorKind`, `grid::Anchors` | positions that follow content | [selection](selection.md) |
| `grid::ScrollRegion`, `grid::ScrollReport`, `grid::TabStops` | scroll margins, what a scroll did, tab stops | [grid](grid.md) |
| `grid::LineClear`, `grid::DisplayClear` | the extents of `EL` and `ED` | [grid](grid.md) |
| constants | `DEFAULT_SCROLLBACK`, `SCROLLBACK_MAX` | [grid](grid.md) |

## `search`

| Item | What it is for | Chapter |
|---|---|---|
| `search::GridText` | a copy of the scrollback text you search without the lock | [search](search.md) |
| `search::SearchPattern`, `search::SearchOptions` | literal or regex (`regex` feature); case and word options | [search](search.md) |
| `search::SearchMatch` | one hit, across wrapped rows | [search](search.md) |
| function, constant | `search_grid_text`; `MAX_MATCHES` | [search](search.md) |

## `input`

| Item | What it is for | Chapter |
|---|---|---|
| `input::KeySpec`, `input::NamedKey`, `input::KeyMods` | what was pressed and with which modifiers | [input](input.md) |
| `input::KeyEvent`, `input::KeyEventKind` | a key press, repeat or release with text | [input](input.md) |
| `input::TerminalMouseButton`, `input::MouseModifiers` | mouse button and modifiers for the encoders | [input](input.md) |
| functions | `encode_key`, `encode_key_event`, `encode_mouse_press`, `encode_mouse_release`, `encode_mouse_move`, `encode_wheel_event` | [input](input.md) |

## `pty` (feature `pty`)

| Item | What it is for | Chapter |
|---|---|---|
| `pty::PseudoConsole` | a shell behind ConPTY or `openpty`, polled by you | [pty](pty.md) |
| `pty::Options`, `pty::Shell`, `pty::WindowSize`, `pty::GlyphWidth` | how to start it: program, environment, size, glyph widths | [pty](pty.md) |
| `pty::EventedReadWrite`, `pty::EventedPty` | pollable read and write; plus the child exit | [pty](pty.md) |
| `pty::OnResize`, `pty::ChildEvent` | resize the console; what happened to the child | [pty](pty.md) |
| `pty::SignalMask` | signal mask the child starts with (Unix only) | [pty](pty.md) |
| `pty::PipeReader`, `pty::PipeWriter` | console output and input as `Read` and `Write` (Windows only) | [pty](pty.md) |
| constants | `PTY_READ_WRITE_TOKEN`, `PTY_CHILD_EVENT_TOKEN` | [pty](pty.md) |
