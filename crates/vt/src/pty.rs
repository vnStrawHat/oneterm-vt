//! A shell behind a pseudo-console, pollable with `polling`.
//!
//! [`PseudoConsole::spawn`] starts a program behind ConPTY (Windows 10 1809 or later) or an
//! `openpty` pair (Unix) and returns a passive object: no read loop, no grid, no knowledge of
//! the VT protocol. The embedder owns a `polling::Poller`, registers the console on it and
//! reads, writes and resizes from its own thread. Two poll keys are used:
//! [`PTY_READ_WRITE_TOKEN`] for input and output, [`PTY_CHILD_EVENT_TOKEN`] for the child's
//! exit.
//!
//! `polling` is this module's only public dependency (its `Poller`, `Event` and `PollMode`
//! appear in [`EventedReadWrite`]); add `polling = "3"` to the embedder's manifest so the types
//! unify. Everything the platforms share is public here; what differs is described in prose:
//!
//! - **Unix**: the master side is a non-blocking `std::fs::File`, so `Reader` and `Writer` are
//!   `File`. `SignalMask` and `Options::child_signal_mask` exist only there. One reaper thread
//!   waits for the child. Dropping the console closes the master, which hangs the session up;
//!   nothing is ever signalled, and a child that ignores `SIGHUP` keeps running.
//! - **Windows**: `Reader` is `PipeReader` and `Writer` is `PipeWriter`, backed by two threads
//!   and 1 MiB buffers that post completion packets to the poller. `Options::escape_args`
//!   exists only there. Dropping the console closes it, waits up to two seconds for the child
//!   to leave, then terminates it, so `drop` belongs on an owner thread, not a UI thread. On
//!   hosts before build 26100 `drop` can also block while `ClosePseudoConsole` waits for a
//!   client that never disconnects.
//!
//! # Reading and writing
//!
//! Both sides follow POSIX `read()` and `write()` on a non-blocking descriptor:
//!
//! | `read` returns | Meaning |
//! |---|---|
//! | `Ok(n)`, `n > 0` | output |
//! | `Err(WouldBlock)` | drained for now: wait for the next poll event |
//! | `Ok(0)`, or `Err` with the raw OS error `EIO` (Linux) | output ended: stop reading |
//!
//! A write may be short, and fails with `WouldBlock` when there is no room at all; keep the rest
//! and retry on a writable event. `write_all` therefore fails under back-pressure.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::Arc;

use polling::{Event, PollMode, Poller};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{PseudoConsole, SignalMask};
#[cfg(windows)]
pub use windows::{PipeReader, PipeWriter, PseudoConsole};

#[cfg(test)]
mod loopback_tests;

/// Poll key of child events ([`ChildEvent`]).
pub const PTY_CHILD_EVENT_TOKEN: usize = 1;
/// Poll key of reads and writes on the console.
pub const PTY_READ_WRITE_TOKEN: usize = 2;

/// Environment variables of the launching terminal: its identity and single-use startup tokens
/// are false in a new console, so the parent's values are not inherited. An entry in
/// [`Options::env`] with one of these names is still applied. Exact names on Unix,
/// ASCII-case-insensitive on Windows.
pub(crate) const DROPPED_PARENT_ENV: [&str; 11] = [
    "WT_SESSION",
    "WT_PROFILE_ID",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERMINAL_EMULATOR",
    "ITERM_SESSION_ID",
    "ITERM_PROFILE",
    "ConEmuPID",
    "ConEmuANSI",
    "XDG_ACTIVATION_TOKEN",
    "DESKTOP_STARTUP_ID",
];

/// How the console host measures the width of a glyph (Windows; ignored on Unix).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlyphWidth {
    /// Per-scalar `wcswidth` widths: matches a session with grapheme clustering (`?2027`) off,
    /// the engine default.
    #[default]
    WcsWidth,
    /// Grapheme-cluster widths: matches a session that turns `?2027` on.
    Graphemes,
}

/// The program to run, passed verbatim: nothing is split or interpreted here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shell {
    program: String,
    args: Vec<String>,
}

impl Shell {
    /// A program and its arguments. The default `Shell` has an empty program, which fails to
    /// spawn with the OS error.
    pub fn new(program: String, args: Vec<String>) -> Self {
        Self { program, args }
    }

    pub(crate) fn program(&self) -> &str {
        &self.program
    }

    pub(crate) fn args(&self) -> &[String] {
        &self.args
    }
}

/// How [`PseudoConsole::spawn`] starts the child.
///
/// Build it with `..Options::default()`, so a field that exists on one platform only never has
/// to be named.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// The program. `None`: `$SHELL`, else `/bin/sh` on Unix; `powershell` on Windows.
    /// On Unix the arguments go to `exec` as they are. On Windows they are joined into one
    /// command line (see `escape_args`).
    pub shell: Option<Shell>,
    /// The child's working directory. Unix applies it best effort (an invalid path leaves the
    /// inherited directory); Windows hands it to `CreateProcessW`, which fails the spawn on an
    /// invalid path.
    pub working_directory: Option<PathBuf>,
    /// Variables added to the parent's environment. The parent's launching-terminal identity
    /// variables (`WT_SESSION`, `TERM_PROGRAM`, `ITERM_SESSION_ID`, startup tokens and
    /// similar) are removed first; an entry here with such a name is applied. The crate never
    /// sets `TERM` or `COLORTERM`: put them here. An empty key, `=` or NUL in a key, or NUL in
    /// a value fails `spawn` with `InvalidInput`.
    pub env: HashMap<String, String>,
    /// Glyph width rule of the console host (Windows only; ignored on Unix).
    pub glyph_width: GlyphWidth,
    /// Unix only. Signal mask set in the child before `exec`; `None` keeps the mask inherited
    /// from the spawning thread. Capture one with `SignalMask::current` on a thread where
    /// terminal signals are unblocked, or Ctrl-C is lost.
    #[cfg(unix)]
    pub child_signal_mask: Option<SignalMask>,
    /// Windows only. `false` (default) joins the arguments verbatim, so the embedder owns the
    /// quoting; `true` applies the C-runtime quoting rules and quotes the program when it
    /// contains a space or tab.
    #[cfg(windows)]
    pub escape_args: bool,
}

/// The size of the console in cells, plus the pixel size of one cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowSize {
    /// Rows. On Windows at most 32767.
    pub rows: u16,
    /// Columns. On Windows at most 32767.
    pub cols: u16,
    /// Width of one cell in pixels (Unix reports `cols * cell_width`; Windows ignores it).
    pub cell_width: u16,
    /// Height of one cell in pixels (Unix reports `rows * cell_height`; Windows ignores it).
    pub cell_height: u16,
}

/// What happened to the child.
#[derive(Debug, PartialEq, Eq)]
pub enum ChildEvent {
    /// The child exited. `None`: the exit code could not be read. Either way the session is
    /// over; output may still be buffered.
    Exited(Option<ExitStatus>),
}

/// A pollable source of output and sink of input.
pub trait EventedReadWrite {
    /// Where output is read from.
    type Reader: io::Read;
    /// Where input is written to.
    type Writer: io::Write;

    /// Registers the console on `poller`: I/O under [`PTY_READ_WRITE_TOKEN`] with `interest`'s
    /// readable and writable flags, child events under [`PTY_CHILD_EVENT_TOKEN`].
    ///
    /// # Safety
    ///
    /// The sources must be deregistered ([`deregister`](Self::deregister)) before `self` or the
    /// poller is dropped, as `Poller::add_with_mode` requires.
    #[expect(
        unsafe_code,
        reason = "register is unsafe because polling::Poller::add_with_mode is"
    )]
    unsafe fn register(
        &mut self,
        poller: &Arc<Poller>,
        interest: Event,
        mode: PollMode,
    ) -> io::Result<()>;

    /// Changes the interest of an already registered console (the child side is unchanged).
    fn reregister(
        &mut self,
        poller: &Arc<Poller>,
        interest: Event,
        mode: PollMode,
    ) -> io::Result<()>;

    /// Removes the console from `poller`.
    fn deregister(&mut self, poller: &Arc<Poller>) -> io::Result<()>;

    /// The output side.
    fn reader(&mut self) -> &mut Self::Reader;

    /// The input side.
    fn writer(&mut self) -> &mut Self::Writer;
}

/// An [`EventedReadWrite`] that also reports its child's exit.
pub trait EventedPty: EventedReadWrite {
    /// The next child event, or `None`. Call it after a wake on [`PTY_CHILD_EVENT_TOKEN`]; each
    /// event is returned exactly once.
    fn next_child_event(&mut self) -> Option<ChildEvent>;
}

/// A console whose size can change.
pub trait OnResize {
    /// Tells the child the new size. A failure leaves the session usable at the old size.
    fn on_resize(&mut self, size: WindowSize) -> io::Result<()>;
}

/// Rejects an environment entry the OS would silently cut or misread.
pub(crate) fn validate_env(env: &HashMap<String, String>) -> io::Result<()> {
    for (key, value) in env {
        if key.is_empty() || key.contains(['=', '\0']) || value.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid environment entry '{key}'"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_validation() {
        let entry = |k: &str, v: &str| HashMap::from([(k.to_owned(), v.to_owned())]);
        assert!(validate_env(&entry("TERM", "xterm-256color")).is_ok());
        assert!(validate_env(&entry("EMPTY", "")).is_ok());
        for bad in [
            entry("", "v"),
            entry("A=B", "v"),
            entry("A\0B", "v"),
            entry("K", "v\0"),
        ] {
            let err = validate_env(&bad).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
            assert!(err.to_string().starts_with("invalid environment entry '"));
        }
    }

    #[test]
    fn bad_env_fails_spawn_before_anything_starts() {
        let options = Options {
            env: HashMap::from([("A=B".to_owned(), "v".to_owned())]),
            ..Options::default()
        };
        let size = WindowSize {
            rows: 24,
            cols: 80,
            ..WindowSize::default()
        };
        let cause = PseudoConsole::spawn(&options, size).err().unwrap();
        assert_eq!(cause.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn pseudo_console_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<PseudoConsole>();
    }
}
