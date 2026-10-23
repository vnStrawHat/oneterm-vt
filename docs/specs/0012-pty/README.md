# 0012: PTY (phase 11)

**Goal:** `oneterm_vt::pty` behind the default `pty` feature: a shell behind ConPTY (Windows) or
`openpty` (Unix), a passive object the embedder polls with its own `polling::Poller` (two tokens:
I/O, child exit), resized with `OnResize`. No read loop, no grid, no VT knowledge.

**Non-goals:** async; shipping a console host; signalling Unix children; job objects; Windows < 10 1809; other transports.

| File | Covers |
|---|---|
| [api.md](api.md) | every public item, traits, `polling` types, reader/writer contract, `Options`, errors |
| [unix.md](unix.md) | `openpty` + `Command`, child side, reaper thread, registration, EIO, resize, drop |
| [windows.md](windows.md) | host selection, spawn steps, command-line escaping, env block, pipe rings, exit watcher, drop order |
| [safety.md](safety.md) | `unsafe` lifts with reasons, every block's invariant, handle/fd ownership and drop order |
| [tests.md](tests.md) | unit, loopback, real-shell and contract tests; tree check; `docs/user/pty.md` outline |
| [files.md](files.md) | files, manifests and features, `lib.rs`, CI, docs, commit |

**Public API** (nothing dropped): `pty::{PseudoConsole (spawn, child_pid), Options, Shell (new), WindowSize,
GlyphWidth, ChildEvent, EventedReadWrite, EventedPty, OnResize, PTY_CHILD_EVENT_TOKEN, PTY_READ_WRITE_TOKEN}`;
Unix `SignalMask (current)`, `Options::child_signal_mask`; Windows `PipeReader`, `PipeWriter`, `Options::escape_args`.

## Acceptance criteria

- [ ] Quality gate; `cargo test -p oneterm-vt --no-default-features`; `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features` on each OS
- [ ] Every test in tests.md passes on Linux, macOS and Windows CI (spawn, read, write, resize, exit, env, cwd, drop)
- [ ] CI tree check: `--no-default-features` lists exactly `bitflags log memchr rustc-hash unicode-segmentation unicode-width`
- [ ] `unsafe` only where safety.md lifts it, each block with a `// SAFETY:` invariant
- [ ] `docs/user/pty.md` exists, its poll loop compiles as a doctest; row 10 `ready`; versioning.md line
- [ ] Commit: `feat(pty): pseudo-console over ConPTY and openpty, pollable with polling` (minor)

## Decisions (settled from POSIX, Microsoft Learn, `polling` docs, specs 0001/0007)

1. `polling` is the one public dependency, not re-exported; `&Arc<Poller>` because Windows threads post packets.
2. Unix: `openpty` + `Command::pre_exec`, not `forkpty`; exit via one reaper thread + socket pair (no `SIGCHLD` handler, no `waitpid` polling).
3. Unix `Reader = File`: `EIO` (Linux) or `Ok(0)` means output ended; documented, not wrapped.
4. Windows exit: `RegisterWaitForSingleObject` on the process handle, one wake per registration; no job object; drop terminates the child after a 2 s grace.
5. Windows I/O: blocking pipe threads + 1 MiB rings + posted completion packets (anonymous pipes cannot join the IOCP).
6. Host: `conpty.dll` beside the executable first, inbox API as fallback; the crate ships no host.
7. `log` becomes a plain dependency (the roadmap's six leaf crates); only `pty` logs today.
8. Enums and structs exhaustive; `Options` built with `..Options::default()` like `Config` (0007 D18).

## Design choices with alternatives (A = as proposed, B = the simpler alternative)

| # | Proposed (A) | Simpler alternative (B) | Evidence |
|---|---|---|---|
| D1 | `FD_CLOEXEC` on master and slave right after `openpty` | keep the fds as `openpty` returns them; the child closes its copies in `pre_exec` | openpty(3) sets no close-on-exec; POSIX `exec` keeps fds without `FD_CLOEXEC` open, so session B's shell holds session A's master and dropping A never hangs A up (unix.md drop contract) |
| D2 | Windows `PipeReader` empty -> `WouldBlock`, `Ok(0)` only at EOF; `PipeWriter` full -> `WouldBlock` | `Ok(0)` means "nothing buffered" / "no room" | POSIX `read()`/`write()` with `O_NONBLOCK`: `EAGAIN` when no data or room, `0` at end of file; the Unix half does exactly that, so under B a portable "`Ok(0)` = ended" loop closes live Windows sessions |
| D3 | drop order reader, console, writer, watcher; the reader thread drains and discards during the close | console first, reader thread stops pushing once its 1 MiB ring is full | Microsoft Learn `ClosePseudoConsole`: before build 26100 it waits indefinitely unless the output pipe is closed or drained; under B a caller that stopped reading hangs its drop forever |
| D4 | env entries validated (`InvalidInput`); Windows block sorted case-insensitively | custom then parent entries in `HashMap` order, unchecked | Microsoft Learn "Changing Environment Variables": strings "must be sorted alphabetically by name ... case-insensitive", `=` not allowed in a name; a NUL silently cuts the entry |
| D5 | with `escape_args`, the program is quoted when it holds a space or tab | program always verbatim | Microsoft Learn `CreateProcessW` security remarks: unquoted `C:\Program Files\MyApp` runs `C:\Program.exe` if it exists |
| D6 | rows or cols above 32767 -> `InvalidInput` on Windows | `as i16` cast | `COORD` fields are `SHORT`: 40000 wraps negative and the host gets a different request |
| D7 | `windows-sys = "0.61"` | `0.59` as pinned by spec 0001 | `polling` 3.11's manifest requires `windows-sys` 0.61; B compiles two copies on Windows |

Rows are independent: B on a row reverts only that row's rules and tests.

## Open items

None. Ruling on the pre-26100 Windows host question: document that `drop` can block while
`ClosePseudoConsole` waits for a client that never disconnects (hosts before build 26100); the
upgrade path, if it ever matters, is closing the console on a detached thread while the
grace/terminate step runs. The coder adds that sentence to `docs/user/pty.md` and `windows.md`.
