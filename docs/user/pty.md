# Pseudo-console

`oneterm_vt::pty::PseudoConsole` starts a shell behind ConPTY (Windows) or `openpty` (Unix) and is
polled with your own `polling::Poller`. It is passive: no read loop, no grid, no VT knowledge. You feed
its bytes to `Terminal::feed` and write replies and keys back. `pty::Registered` puts it on the poller
in safe code.

## Example: spawn, poll, feed

```rust,no_run
#![forbid(unsafe_code)] // nothing here needs it
use std::io::{self, ErrorKind, Read, Write};
use std::sync::Arc;
use std::time::Instant;
use oneterm_vt::pty::{
    ChildEvent, Options, PseudoConsole, Registered, WindowSize, PTY_CHILD_EVENT_TOKEN,
    PTY_READ_WRITE_TOKEN,
};
use oneterm_vt::{Config, EventBatch, Size, Terminal, VtEvent};
use polling::{Event, Events, PollMode, Poller};

// Read until `WouldBlock`; `true` when the output ended.
fn drain(pty: &mut Registered, term: &mut Terminal, batch: &mut EventBatch) -> io::Result<bool> {
    let mut buf = [0u8; 8192];
    loop {
        let n = match pty.read(&mut buf) {
            Ok(0) => return Ok(true),
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.raw_os_error() == Some(5) => return Ok(true), // EIO: Linux's "ended"
            Err(e) => return Err(e),
        };
        term.feed(&buf[..n], batch, Instant::now());
        for event in batch.iter() {
            if let VtEvent::Reply(span) = event {
                pty.write_all(batch.bytes(*span))?; // real code keeps short writes
            }
        }
    }
}

let env = [("TERM".to_owned(), "xterm-256color".to_owned())];
let options = Options { env: env.into(), ..Options::default() };
let size = WindowSize { rows: 24, cols: 80, cell_width: 8, cell_height: 16 };
let pty = PseudoConsole::spawn(&options, size)?;
let mut term = Terminal::new(Size { rows: 24, cols: 80 }, Config::default());
let mut batch = EventBatch::new();
let poller = Arc::new(Poller::new()?);
// Owns the console, keeps the poller alive, deregisters when dropped.
let interest = Event::readable(PTY_READ_WRITE_TOKEN);
let mut pty = Registered::new(pty, Arc::clone(&poller), interest, PollMode::Level)?;

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
# let _ = status;
# Ok::<(), io::Error>(())
```

## You owe the console

- `polling = "3"` in your own manifest: its `Poller`, `Event` and `PollMode` are in the API.
- Drop the registration on an owner thread, not a UI thread: it drops the console.
- For writes, `reregister` with a readable and writable interest while bytes are pending. From other
  threads, queue commands and wake the owner with `poller().notify()`: `wait` returns with no event.
- Read until `WouldBlock`; after the child token wakes you, read once more. An ended master keeps waking a
  level poll on Linux (hang-up is always reported); the exit event follows right after, so stop reading and wait for it.
- Write `VtEvent::Reply` bytes and encoded keys with `write`; keep what a short write left.
- `on_resize` with the numbers you give `Terminal::resize`; `console().child_pid()` for the pid.
- `TERM` and `COLORTERM` in `Options::env`: the crate never sets them.

## Who keeps the registration

| | `Registered::new(console, poller, interest, mode)` | `EventedReadWrite::register` |
|---|---|---|
| deregisters before the console or the poller is dropped | the engine, when the registration is dropped | you: this is why it is `unsafe` |
| the poller outlives the console | the engine: the registration holds the `Arc` | you |
| reads and writes | `Read` and `Write` on the registration | `reader()` and `writer()` |
| the loop, the queue of pending writes, the threads | you | you |

`register` stays for a console you must take back out of a poller; `Registered` never gives its
console back. It takes any `EventedReadWrite` of your own too.

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

Default shell, arguments, working directory, what `drop` does, the crate's threads, the Windows console
host and building without `pty`: [pty-platforms](pty-platforms.md).
