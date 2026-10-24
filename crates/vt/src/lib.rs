//! `oneterm-vt`: an embeddable terminal core.
//!
//! Bytes in; a grid, events and damage-tracked snapshots out. No rendering, no policy,
//! no lock, no global state. Terminal input is untrusted: nothing here panics on it or
//! returns an error for it. Malformed input is dropped, truncated or degraded, and counted.
//!
//! The engine is being rebuilt module by module; today the public surface is [`parser`], a
//! byte-stream parser that reports control sequences to a [`parser::Dispatch`] sink, the
//! storage and measurement of one grid position: [`Cell`], [`Style`], the [`intern`] tables
//! a cell points into, and the [`scalar_width`] / [`cluster_width`] column rules, and the
//! [`grid`]: rows in a ring-buffer scrollback, primary and alternate screens, and the
//! anchors that follow content, with column reflow and row resize, and the [`Selection`]s
//! that follow that content. A [`Terminal`] ties them together: bytes in, grid changes, events
//! and per-call counters out, and damage-tracked [`SnapshotState`]s that copy only the rows
//! that changed, with synchronized output (`?2026`) holding frames while a program redraws, and
//! [`search`]: literal and regular-expression scrollback search over a copy of the grid,
//! matches spanning wrapped rows. [`input`] turns key and mouse events into the bytes the
//! program expects, honouring DECCKM, the kitty keyboard protocol, `modifyOtherKeys` and the
//! mouse modes. Sixel images are decoded to RGBA, anchored to the cells they cover and released
//! with an event when those cells are gone. With the default `pty` feature, [`pty`] starts a
//! shell behind a pseudo-console (ConPTY on Windows, `openpty` on Unix) that an embedder polls
//! with `polling`.

#![warn(missing_docs)]

mod cell;
mod events;
mod graphics;
pub mod grid;
pub mod input;
pub mod intern;
pub mod parser;
#[cfg(feature = "pty")]
pub mod pty;
pub mod search;
mod selection;
mod snapshot;
mod terminal;
mod width;

pub use cell::{Attrs, Cell, CellContent, CellWidth, Color, NamedColor, Rgb, Semantic, Style};
pub use events::{
    ByteSpan, ClipboardKind, EventBatch, FeedStats, ParamSpans, Progress, ShellMark, StrSpan,
    VtEvent,
};
pub use graphics::{GraphicData, Placement, VIRTUAL_CELL};
pub use grid::terminal_grid::{ResizeOutcome, ResizePolicy};
pub use grid::{Pos, RowId, RowRef, SeqNo, Size, Viewport};
pub use parser::StringTerm;
pub use selection::{Invalidation, Selection, SelectionKind, SelectionRange, Side};
pub use snapshot::{
    ModeSnapshot, MouseEncoding, MouseProtocol, MouseReporting, Palette, SnapshotCell,
    SnapshotContent, SnapshotCursor, SnapshotPlacement, SnapshotRow, SnapshotState, SnapshotUpdate,
    StyleRun, SyncState, Watermark,
};
pub use terminal::{
    ColorKey, ColorOverrides, Config, CursorShape, CursorStyle, KeyboardFlags, Mode, ModeState,
    OscRoute, OscRoutes, Terminal,
};
pub use width::{cluster_width, scalar_width};

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/parser.md")]
mod user_guide_parser {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/cells-and-styles.md")]
mod user_guide_cells_and_styles {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/grid.md")]
mod user_guide_grid {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/resize.md")]
mod user_guide_resize {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/selection.md")]
mod user_guide_selection {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/events.md")]
mod user_guide_events {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/limits.md")]
mod user_guide_limits {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/conformance.md")]
mod user_guide_conformance {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/snapshot.md")]
mod user_guide_snapshot {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/search.md")]
mod user_guide_search {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/threading.md")]
mod user_guide_threading {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/quickstart.md")]
mod user_guide_quickstart {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/input.md")]
mod user_guide_input {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/graphics.md")]
mod user_guide_graphics {}

#[cfg(all(doctest, feature = "pty"))]
#[doc = include_str!("../../../docs/user/pty.md")]
mod user_guide_pty {}
