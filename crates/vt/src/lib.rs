//! `oneterm-vt`: an embeddable terminal core.
//!
//! Bytes in; a grid, events and damage-tracked snapshots out. No rendering, no policy,
//! no lock, no global state. Terminal input is untrusted: nothing here panics on it or
//! returns an error for it. Malformed input is dropped, truncated or degraded, and counted.
//!
//! The engine is being rebuilt module by module; today the public surface is [`parser`], a
//! byte-stream parser that reports control sequences to a [`parser::Dispatch`] sink, and the
//! storage and measurement of one grid position: [`Cell`], [`Style`], the [`intern`] tables
//! a cell points into, and the [`scalar_width`] / [`cluster_width`] column rules.

#![warn(missing_docs)]

mod cell;
pub mod intern;
pub mod parser;
mod width;

pub use cell::{Attrs, Cell, CellContent, CellWidth, Color, NamedColor, Rgb, Semantic, Style};
pub use width::{cluster_width, scalar_width};

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/parser.md")]
mod user_guide_parser {}

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/cells-and-styles.md")]
mod user_guide_cells_and_styles {}
