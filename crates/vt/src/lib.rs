//! `oneterm-vt`: an embeddable terminal core.
//!
//! Bytes in; a grid, events and damage-tracked snapshots out. No rendering, no policy,
//! no lock, no global state. Terminal input is untrusted: nothing here panics on it or
//! returns an error for it. Malformed input is dropped, truncated or degraded, and counted.
//!
//! The engine is being rebuilt module by module; this release has no public items yet.

#![warn(missing_docs)]
