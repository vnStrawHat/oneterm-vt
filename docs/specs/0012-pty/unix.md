# Unix: `src/pty/unix.rs` (`openpty` + `std::process::Command`)

```rust
pub struct PseudoConsole {
    master: File,                          // non-blocking, FD_CLOEXEC; reader and writer
    exit_signal: UnixStream,               // non-blocking; readable once the child exited
    exit_events: mpsc::Receiver<ChildEvent>,
    pid: u32,
}
```

## `spawn(options, size)`, parent side, in order

| # | Step | On failure |
|---|---|---|
| 1 | validate `options.env` (api.md, **D4**) | `InvalidInput` |
| 2 | `openpty(&mut m, &mut s, null, null, null)`; wrap both in `OwnedFd` | `openpty failed` |
| 3 | `fcntl(F_SETFD, FD_CLOEXEC)` on master and slave (**D1**) | error |
| 4 | best effort: `tcgetattr`, `c_iflag |= IUTF8`, `tcsetattr(TCSANOW)` on the master (Linux, macOS only) | ignored |
| 5 | `TIOCSWINSZ` on the master with `size` (resize below) | resize error |
| 6 | `Command::new(program)` + `args`; default program `$SHELL`, else `/bin/sh` | |
| 7 | stdin/stdout/stderr = `Stdio::from(slave.try_clone()?)` each (std dups with CLOEXEC; `dup2` onto 0-2 in the child clears it) | error |
| 8 | env: `env_remove` each `DROPPED_PARENT_ENV` name, then `env(k, v)` per entry | |
| 9 | `pre_exec` closure (child side, below); captures only `Copy`/pre-built values | |
| 10 | `command.spawn()`; error -> `cannot start '<program>': <e>` (kind kept) | error |
| 11 | `pid = child.id()`; `fcntl(F_GETFL)` then `F_SETFL | O_NONBLOCK` on the master | error |
| 12 | `UnixStream::pair()` -> `(exit_signal, waker)`; `exit_signal.set_nonblocking(true)` | error |
| 13 | `mpsc::channel()`; spawn the reaper thread (below) | `io::Error` from `Builder::spawn` |
| 14 | return; `slave` and the `Command` (its stdio dups) drop here, so only the child holds the slave | |

`Command` with a `pre_exec` closure always uses `fork` + `exec` (std skips `posix_spawn`), and
reports `exec` failure (missing program) through its own CLOEXEC pipe: no hand-rolled `forkpty`.

## Child side (`pre_exec`, after std's `dup2` of stdio and before `exec`)

Async-signal-safe calls only; no allocation (the `CString` cwd and the mask are built in the parent).

1. `setsid()`: a new session; `-1` -> `Err(last_os_error)` (spawn fails).
2. cwd: `chdir(cstr)` if `working_directory` converted to a `CString` (interior NUL -> skipped);
   the result is ignored (best effort, api.md).
3. `ioctl(slave_fd, TIOCSCTTY as _, 0)`: the slave becomes the controlling terminal; error -> spawn fails.
4. no explicit `close`: master and slave carry `FD_CLOEXEC` (**D1**) and close at `exec`.
5. `child_signal_mask`: `sigprocmask(SIG_SETMASK, &mask, null)`; error -> spawn fails. `None`
   keeps the inherited mask (std resets only `SIGPIPE`).
6. `signal(s, SIG_DFL)` for `SIGCHLD SIGHUP SIGINT SIGQUIT SIGTERM SIGALRM` (embedder handlers
   are not the shell's).

## Child exit: a reaper thread (decision)

- Thread `oneterm-vt-pty-reaper` owns the `Child`: `child.wait()`, then
  `send(ChildEvent::Exited(status.ok()))`, then `write_all(&[1])` on `waker`. Event before wake,
  so a poll woken by the byte always finds the event. Both results ignored (the session may be gone).
- Not joined; it outlives the `PseudoConsole` until the child exits. It holds only the `Child`,
  the `Sender` and `waker`.
- Rejected: a `SIGCHLD` handler (process-global; a library must not claim it) and `waitpid`
  polling (needs a timer, latency). Linux `pidfd`/kqueue `EVFILT_PROC` are platform splits for
  no gain. If the embedder sets `SIGCHLD` to `SIG_IGN`, `wait` fails (`ECHILD`) -> `Exited(None)`.
- `next_child_event`: read one byte from `exit_signal` (`WouldBlock` fine; other errors
  `log::error!`), then `exit_events.try_recv().ok()`. Reading the byte first stops a
  level-triggered poll from spinning; the event is reported exactly once.

## Polling registration

| Method | Master (`PTY_READ_WRITE_TOKEN`) | `exit_signal` |
|---|---|---|
| `register` (unsafe) | `poller.add_with_mode(&master, interest, mode)` | `add_with_mode(.., Event::readable(PTY_CHILD_EVENT_TOKEN), Level)` |
| `reregister` | `modify_with_mode(&master, interest, mode)` | `modify_with_mode(.., same, Level)` |
| `deregister` | `delete(&master)` | `delete(&exit_signal)` |

`reader()` and `writer()` both return `&mut self.master`.

## Reading, writing, end of output

- Empty master: `EAGAIN` -> `WouldBlock`. Full: short write or `WouldBlock`.
- After the child and every process holding the slave are gone: Linux `read` returns `EIO`;
  other Unixes may return `Ok(0)`. Both mean "output ended" (api.md). `File` is returned as is;
  no wrapper maps `EIO` (the associated type stays `File`).
- Bytes still in the master after `Exited`: the embedder drains until `WouldBlock`/end
  (docs/user/pty.md).

## Resize: `on_resize(size)`

`winsize { ws_row: rows, ws_col: cols, ws_xpixel: cols.saturating_mul(cell_width), ws_ypixel:
rows.saturating_mul(cell_height) }`, `ioctl(master, TIOCSWINSZ, &ws)`; the kernel sends `SIGWINCH`
to the foreground process group. Failure -> `ioctl TIOCSWINSZ to {cols}x{rows} failed: <e>`.

## Drop (no `Drop` impl)

Dropping closes the master. The slave's last holders are the child's process tree, so the close
hangs up the controlling terminal and the session leader gets `SIGHUP` (POSIX `close()` on a
pseudo-terminal master). The crate never `kill`s: the reaper may already have reaped the pid,
which could then name another process. The drop does not block. A child that ignores `SIGHUP`
keeps running, and its reaper with it (rustdoc says so).
