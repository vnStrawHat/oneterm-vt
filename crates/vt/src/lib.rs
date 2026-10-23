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
//! and per-call counters out.

#![warn(missing_docs)]

mod cell;
mod events;
pub mod grid;
pub mod intern;
pub mod parser;
mod selection;
mod terminal;
mod width;

pub use cell::{Attrs, Cell, CellContent, CellWidth, Color, NamedColor, Rgb, Semantic, Style};
pub use events::{
    ByteSpan, ClipboardKind, EventBatch, FeedStats, ParamSpans, Progress, ShellMark, StrSpan,
    VtEvent,
};
pub use grid::terminal_grid::{ResizeOutcome, ResizePolicy};
pub use grid::{Pos, RowId, RowRef, SeqNo, Size, Viewport};
pub use parser::StringTerm;
pub use selection::{Invalidation, Selection, SelectionKind, SelectionRange, Side};
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
