# Pseudo-console

`oneterm_vt::pty::PseudoConsole` starts a shell behind ConPTY (Windows) or `openpty` (Unix) and is
polled with your own `polling::Poller`. It is passive: no read loop, no grid, no VT knowledge. You feed
its bytes to `Terminal::feed` and write replies and keys back.

## Example: spawn, poll, feed

```rust,no_run
#![allow(unsafe_code)] // only `register` is unsafe; a deny-unsafe crate must allow it
use std::io::{self, ErrorKind, Read, Write};
use std::sync::Arc;
use std::time::Instant;
use oneterm_vt::pty::{
    ChildEvent, EventedPty, EventedReadWrite, Options, PseudoConsole, WindowSize,
    PTY_CHILD_EVENT_TOKEN, PTY_READ_WRITE_TOKEN,
};
use oneterm_vt::{Config, EventBatch, Size, Terminal, VtEvent};
use polling::{Event, Events, PollMode, Poller};

// Read until `WouldBlock`; `true` when the output ended.
fn drain(pty: &mut PseudoConsole, term: &mut Terminal, batch: &mut EventBatch) -> io::Result<bool> {
    let mut buf = [0u8; 8192];
    loop {
        let n = match pty.reader().read(&mut buf) {
            Ok(0) => return Ok(true),
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.raw_os_error() == Some(5) => return Ok(true), // EIO: Linux's "ended"
            Err(e) => return Err(e),
        };
        term.feed(&buf[..n], batch, Instant::now());
        for event in batch.iter() {
            if let VtEvent::Reply(span) = event {
                pty.writer().write_all(batch.bytes(*span))?; // real code keeps short writes
            }
        }
    }
}

let env = [("TERM".to_owned(), "xterm-256color".to_owned())];
let options = Options { env: env.into(), ..Options::default() };
let size = WindowSize { rows: 24, cols: 80, cell_width: 8, cell_height: 16 };
let mut pty = PseudoConsole::spawn(&options, size)?;
let mut term = Terminal::new(Size { rows: 24, cols: 80 }, Config::default());
let mut batch = EventBatch::new();
let poller = Arc::new(Poller::new()?);
// SAFETY: deregistered below, before `pty` and `poller` are dropped.
unsafe { pty.register(&poller, Event::readable(PTY_READ_WRITE_TOKEN), PollMode::Level)? };

let (mut events, mut ended) = (Events::new(), false); // `ended`: stop reading, await the exit
let status = 'run: loop {
    events.clear();
    poller.wait(&mut events, None)?;
    for event in events.iter() {
        match event.key {
            PTY_READ_WRITE_TOKEN => {
                if !ended {
                    ended = drain(&mut pty, &mut term, &mut batch)?;
                }
            }
            PTY_CHILD_EVENT_TOKEN => {
                if let Some(ChildEvent::Exited(status)) = pty.next_child_event() {
                    drain(&mut pty, &mut term, &mut batch)?; // bytes may still be buffered
                    break 'run status; // Option<ExitStatus>
                }
            }
            _ => {}
        }
    }
};
pty.deregister(&poller)?;
# Ok::<(), io::Error>(())
```

## You owe the console

- `polling = "3"` in your own manifest: its `Poller`, `Event` and `PollMode` are in the API.
- `deregister` before the poller or the console is dropped (`register` is `unsafe` for this); drop
  the console on an owner thread, not a UI thread.
- Read until `WouldBlock`; after the child token wakes you, read once more. An ended master keeps waking a
  level poll on Linux (hang-up is always reported); the exit event follows right after, so stop reading and wait for it.
- Write `VtEvent::Reply` bytes and encoded keys to `writer()`; keep what a short write left.
- `OnResize::on_resize` with the numbers you give `Terminal::resize`.
- `TERM` and `COLORTERM` in `Options::env`: the crate never sets them.

## Return values

| `read` returns | Meaning | You do |
|---|---|---|
| `Ok(n > 0)` | output | `Terminal::feed` |
| `Err(WouldBlock)` | drained for now | wait for the next poll event |
| `Ok(0)`, or `Err` with raw OS error `EIO` (Linux) | output ended | stop reading; the exit event follows or came |

Writes may be short; `Err(WouldBlock)` means no room (`write_all` fails under back-pressure): keep the
rest, retry on a writable event. POSIX `read()`/`write()` semantics, on every platform.

Read output is never dropped: if you read slower than the child writes, the child is blocked (kernel
pty buffer on Unix; the 1 MiB output ring waits on Windows). Bytes are discarded only after the reader
is dropped.

## Platforms

| | Unix | Windows (10 1809+) |
|---|---|---|
| default shell | `$SHELL`, else `/bin/sh` | `powershell` |
| arguments | passed to `exec` as they are | one command line; `Options::escape_args` (default `false`) quotes them |
| `child_signal_mask` | `SignalMask::current()` on a thread with signals unblocked, else Ctrl-C is lost | n/a |
| working directory | best effort; an invalid path keeps the inherited one | an invalid path fails `spawn` |
| `WindowSize` | pixel fields reported to the child | pixel fields ignored; rows and cols at most 32767 |
| `drop` | closes the master: the session gets `SIGHUP`; never blocks, never signals | closes the console, waits up to ~2 s, then terminates the child; may block on hosts before build 26100 while `ClosePseudoConsole` waits for a client that never disconnects |
| crate threads | 1 (reaper) | 2 (pipes), plus a thread-pool wait; none joined |

A Unix child that ignores `SIGHUP` keeps running. An `Options::env` key must be non-empty without `=`
or NUL and a value without NUL, or `spawn` fails with `InvalidInput`. The parent's terminal identity
(`WT_SESSION`, `TERM_PROGRAM`, `ITERM_SESSION_ID`, startup tokens) is not inherited.

## Console host, and turning `pty` off

`spawn` uses `conpty.dll` beside your executable when present, else the inbox API. Ship a matched
`conpty.dll` and `x64\OpenConsole.exe` next to it: without them the inbox `conhost.exe` swallows
Sixel (see [graphics](graphics.md)); the crate ships neither. `default-features = false` removes
the module, `polling`, `libc` and `windows-sys`, leaving `bitflags`, `log`, `memchr`, `rustc-hash`,
`unicode-segmentation` and `unicode-width`; `Terminal::feed` is the seam for your own transport.
