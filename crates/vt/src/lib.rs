//! `oneterm-vt`: an embeddable terminal core.
//!
//! Bytes in; a grid, events and damage-tracked snapshots out. No rendering, no policy,
//! no lock, no global state. Terminal input is untrusted: nothing here panics on it or
//! returns an error for it. Malformed input is dropped, truncated or degraded, and counted.
//!
//! The engine is being rebuilt module by module; today the public surface is [`parser`], a
//! byte-stream parser that reports control sequences to a [`parser::Dispatch`] sink.

#![warn(missing_docs)]

pub mod parser;

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/parser.md")]
mod user_guide_parser {}
