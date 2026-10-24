//! Key and mouse events to the bytes a program expects.
//!
//! Every encoder here is a pure function of an event and a [`ModeSnapshot`]: no state, no I/O,
//! no lock. The engine knows which modes the program negotiated (`DECCKM`, the kitty keyboard
//! flags, `modifyOtherKeys`, the mouse protocol); the embedder owns the side effects around
//! them (scrolling to the bottom, clearing a selection, shortcuts, writing the bytes to the
//! PTY). The event types are framework-neutral: map your platform's events onto [`KeySpec`],
//! [`KeyMods`] and [`NamedKey`].
//!
//! Keys go through four rungs, first match wins: an unrequested release sends nothing, then the
//! kitty keyboard protocol, then `modifyOtherKeys`, then the legacy (xterm) table. A key
//! encoder returns `None` when the event sends nothing; a mouse encoder returns an empty
//! `Vec`.
//!
//! [`ModeSnapshot`]: crate::ModeSnapshot

mod key;
mod kitty;
mod mouse;

pub use key::{KeyEvent, KeyEventKind, KeyMods, KeySpec, NamedKey, encode_key, encode_key_event};
pub use mouse::{
    MouseModifiers, TerminalMouseButton, encode_mouse_move, encode_mouse_press,
    encode_mouse_release, encode_wheel_event,
};
