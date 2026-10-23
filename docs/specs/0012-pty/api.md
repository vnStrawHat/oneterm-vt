# Public API: `oneterm_vt::pty` (`src/pty.rs`, feature `pty`)

Both platforms unless marked. Nothing `#[non_exhaustive]` (a new item or variant is a version bump
per `docs/user/versioning.md`). No `pub use polling`.

## Items

```rust
pub const PTY_CHILD_EVENT_TOKEN: usize = 1;   // poll key of child events
pub const PTY_READ_WRITE_TOKEN: usize = 2;    // poll key of reads and writes

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlyphWidth { #[default] WcsWidth, Graphemes }

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shell { program: String, args: Vec<String> }
impl Shell { pub fn new(program: String, args: Vec<String>) -> Self }  // verbatim, nothing split

#[derive(Clone, Debug, Default)]                // no PartialEq: `sigset_t` has no portable eq
pub struct Options {
    pub shell: Option<Shell>,                   // None: Unix `$SHELL` else `/bin/sh`; Windows `powershell`
    pub working_directory: Option<PathBuf>,
    pub env: HashMap<String, String>,
    pub glyph_width: GlyphWidth,
    #[cfg(unix)]    pub child_signal_mask: Option<SignalMask>,
    #[cfg(windows)] pub escape_args: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowSize { pub rows: u16, pub cols: u16, pub cell_width: u16, pub cell_height: u16 }

#[derive(Debug, PartialEq, Eq)]
pub enum ChildEvent { Exited(Option<ExitStatus>) }  // None: exit code unreadable; session over

pub trait EventedReadWrite {
    type Reader: io::Read;
    type Writer: io::Write;
    /// # Safety: the sources must be deregistered before `self` or the poller is dropped.
    unsafe fn register(&mut self, poller: &Arc<Poller>, interest: Event, mode: PollMode) -> io::Result<()>;
    fn reregister(&mut self, poller: &Arc<Poller>, interest: Event, mode: PollMode) -> io::Result<()>;
    fn deregister(&mut self, poller: &Arc<Poller>) -> io::Result<()>;
    fn reader(&mut self) -> &mut Self::Reader;
    fn writer(&mut self) -> &mut Self::Writer;
}
pub trait EventedPty: EventedReadWrite { fn next_child_event(&mut self) -> Option<ChildEvent>; }
pub trait OnResize { fn on_resize(&mut self, size: WindowSize) -> io::Result<()>; }

pub struct PseudoConsole { .. }                 // one type per platform, same inherent API
impl PseudoConsole {
    pub fn spawn(options: &Options, size: WindowSize) -> io::Result<Self>;
    pub fn child_pid(&self) -> Option<u32>;     // Unix always Some; Windows None if GetProcessId gave 0
}
// impl EventedPty + OnResize for PseudoConsole on both platforms
```

| Platform-only | Items |
|---|---|
| Unix | `#[derive(Clone, Copy)] pub struct SignalMask(libc::sigset_t)` (private field; manual `Debug` prints `SignalMask`); `pub fn current() -> io::Result<Self>` = `sigemptyset`, then `pthread_sigmask(SIG_SETMASK, null, &mut set)` |
| Unix | `Reader = Writer = std::fs::File` (the master side) |
| Windows | `pub struct PipeReader: io::Read`, `pub struct PipeWriter: io::Write`; no public constructor or method; `Reader = PipeReader`, `Writer = PipeWriter` |

`PseudoConsole` is `Send` (tested on both platforms) without `unsafe impl` (`HPCON` is an `isize`); no `Debug`.

## `polling` in the API

- `polling::{Poller, Event, PollMode}` appear in `EventedReadWrite`, so `polling` is the crate's
  only public dependency: a `polling` major bump is a major bump here (versioning.md gains a line).
  The embedder adds `polling = "3"` to its own manifest; the types must unify.
- `&Arc<Poller>`, not `&Poller`: on Windows the pipe threads and the exit callback keep a clone
  and post completion packets (`PollerIocpExt::post`) from their own threads.
- `register` is `unsafe` because `Poller::add_with_mode` is (polling docs: a source must be
  `delete()`d before it is dropped); Windows adds no OS source but shares the signature.
- Keys: `register`/`reregister` set `interest.key = PTY_READ_WRITE_TOKEN` for the I/O side; the
  child side is always `Event::readable(PTY_CHILD_EVENT_TOKEN)` (Unix: `PollMode::Level`; Windows:
  one posted packet per registration, windows.md).

## Reader contract (both platforms; POSIX `read()` semantics, **D2**)

| `read` returns | Meaning | Embedder does |
|---|---|---|
| `Ok(n > 0)` | output | `Terminal::feed` |
| `Err(WouldBlock)` | drained for now | wait for the next poll event |
| `Ok(0)`, or `Err` with `raw_os_error() == Some(EIO)` (Linux) | output ended | stop reading; the exit event follows or came |

Writes, POSIX `write()` semantics on both platforms (**D2**): `Ok(n)` may be short (non-blocking
fd; Windows ring of 1 MiB); `Err(WouldBlock)` when there is no room at all. Keep the rest and
retry on a writable event. `write_all` therefore fails with `WouldBlock` under back-pressure.

## `Options` semantics

- `shell`: Unix passes `argv` through `Command` (no shell parsing); Windows joins one command
  line (windows.md). `Shell::default()` has an empty program: `spawn` fails with the OS error.
- `working_directory`: Unix applies it best effort (invalid path: inherited cwd); Windows hands it
  to `CreateProcessW`, which fails the spawn on an invalid path. Rustdoc states both.
- `env`: parent environment minus `DROPPED_PARENT_ENV`, then every `env` entry on top (an entry
  with a dropped name is applied). The crate never calls `set_var` and never sets `TERM` or
  `COLORTERM`: the embedder puts them in `env`. An empty key, `=` or NUL in a key, or NUL in a
  value fails `spawn` with `InvalidInput` before anything is created (**D4**).
- `DROPPED_PARENT_ENV` (`pub(crate)`; exact on Unix, ASCII-case-insensitive on Windows):
  `WT_SESSION WT_PROFILE_ID TERM_PROGRAM TERM_PROGRAM_VERSION TERMINAL_EMULATOR ITERM_SESSION_ID
  ITERM_PROFILE ConEmuPID ConEmuANSI XDG_ACTIVATION_TOKEN DESKTOP_STARTUP_ID`. Reason (rustdoc):
  the launching terminal's identity and single-use startup tokens are false in a new console.
- `glyph_width`: ConPTY creation flag, fixed at spawn. `WcsWidth` matches the engine default
  (`?2027` off); `Graphemes` matches a session that turns `?2027` on. Ignored on Unix.
- `escape_args` (Windows, default `false`): `false` joins the args verbatim (the embedder owns the
  quoting); `true` applies the C-runtime rules and quotes the program when needed (**D5**).
- `child_signal_mask` (Unix): `Some(mask)` is set in the child before `exec`; `None` keeps the
  mask inherited from the spawning thread (std does not reset it). Rustdoc: capture it with
  `SignalMask::current()` on a thread where terminal signals are unblocked, else Ctrl-C is lost.

## Errors

| Case | Message | Kind |
|---|---|---|
| spawn failed | `cannot start '<program>': <os error>` (Windows default: `the default shell`) | the OS error's |
| ConPTY create | `CreatePseudoConsole failed (HRESULT 0x..)` | `Other` |
| resize | `ioctl TIOCSWINSZ to {cols}x{rows} failed: <os error>` / `ResizePseudoConsole to {cols}x{rows} failed (HRESULT 0x..)` | `Other` |
| bad env entry | `invalid environment entry '<key>'` (**D4**) | `InvalidInput` |
| Windows size > 32767 | `window size {cols}x{rows} exceeds 32767` (**D6**) | `InvalidInput` |
| setup calls (`openpty`, `fcntl`, `CreatePipe`, attribute list, thread spawn) | `<call> failed: <os error>` | the OS error's |

All `io::Error`. A failed resize leaves the session usable at the old size. Nothing panics.
