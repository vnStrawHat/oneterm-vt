# `unsafe`: lifts, invariants, ownership

The workspace denies `unsafe_code` (`deny`, overridable). Phase 11 lifts it only where FFI or
the `unsafe fn register` signature needs it, each with `reason = ".."` (the lint also fires on
declaring and implementing an `unsafe` method). Every `unsafe` block carries `// SAFETY:` naming
the invariant from the tables below. No `unsafe impl` anywhere.

## Lifts

| Where | Lift (reason text in quotes) |
|---|---|
| `src/pty.rs` | `#[allow(unsafe_code, reason = "register is unsafe because polling::Poller::add_with_mode is")]` on `trait EventedReadWrite` only |
| `src/pty/unix.rs` | `#![allow(unsafe_code, reason = "libc FFI: openpty, fcntl, ioctl, termios, signal masks")]` |
| `src/pty/windows.rs` | the same `#[allow(.., reason = "implements the unsafe register")]` on `impl EventedReadWrite for PseudoConsole` only |
| `src/pty/windows/conpty.rs` | `#![allow(unsafe_code, reason = "Win32 FFI: ConPTY, pipes, CreateProcessW, dynamic conpty.dll")]` |
| `src/pty/windows/child.rs` | `#![allow(unsafe_code, reason = "Win32 FFI: process wait callback and termination")]` |
| `src/pty/windows/pipe.rs` | `#![allow(unsafe_code, reason = "Win32 FFI: ReadFile and WriteFile on owned pipe handles")]` |
| `src/pty/loopback_tests.rs` | `#![allow(unsafe_code, reason = "implements and calls register")]` |
| `tests/pty_contract.rs` | `#![allow(unsafe_code, reason = "registers a PseudoConsole with a Poller")]` |
| `docs/user/pty.md` doctest | visible first line `#![allow(unsafe_code)]`: shows embedders what a deny-unsafe crate needs |

No other test module calls `register` (real-shell poll tests live in `tests/pty_contract.rs`).
`unix.rs` and `windows/*.rs` exist only under `cfg(unix)` / `cfg(windows)` and the `pty` feature.

## Unix blocks (`unix.rs`)

| Call | Invariant |
|---|---|
| `openpty(&mut m, &mut s, null, null, null)` | out-params are live `c_int`s; optional args null |
| `OwnedFd::from_raw_fd(m / s)` | `openpty` returned 0, both fds are open and owned by nobody else |
| `fcntl(fd, F_SETFD / F_GETFL / F_SETFL)` | `fd` borrowed from an `OwnedFd` alive for the call |
| `tcgetattr` / `assume_init` / `tcsetattr` | master open; `assume_init` only after `tcgetattr` returned 0 |
| `ioctl(master, TIOCSWINSZ, &ws)` | master open; `ws` lives across the call |
| `Command::pre_exec(closure)` | the closure runs post-fork in a single-threaded child: only async-signal-safe calls (`setsid`, `chdir`, `ioctl`, `sigprocmask`, `signal`); no allocation, no lock; captured values are `Copy` or built before the fork |
| `ioctl(slave_fd, TIOCSCTTY as _, 0)` | in the child after `setsid`; `slave_fd` still open (CLOEXEC closes it only at `exec`) |
| `SignalMask::current`: `sigemptyset`, `pthread_sigmask(SIG_SETMASK, null, p)`, `assume_init` | `sigemptyset` zeroes every byte first (the call writes only a prefix); `assume_init` only after both succeeded |
| `sigprocmask(SIG_SETMASK, &mask.0, null)` | `mask.0` is initialised (only `current` builds one) |
| `send(waker_fd, &byte, 1, MSG_NOSIGNAL)` (linux, android, freebsd, netbsd, openbsd, illumos) | `waker` open for the call; `byte` a live one-byte buffer; `MSG_NOSIGNAL` turns a closed peer into an ignored `EPIPE` instead of `SIGPIPE` |
| `setsockopt(waker_fd, SOL_SOCKET, SO_NOSIGPIPE, &on, size_of::<c_int>())` (Apple) | `waker` open for the call; `on` a live `c_int` whose size is passed |
| `unsafe fn become_session_leader` (called from the `pre_exec` closure) | the caller is the post-fork, single-threaded child, before `exec`, with `slave_fd` open |

## Windows blocks

| Call | Invariant |
|---|---|
| `LoadLibraryW(wide)` | `wide` NUL-terminated, alive for the call; module never freed |
| `GetProcAddress(module, s!("..."))` | `module` non-null and never freed; name NUL-terminated |
| `transmute::<FARPROC fn, CreatePseudoConsoleFn>` (x3) | resolved by name from a module exporting the kernel32 ConPTY ABI (same names, same signatures, `extern "system"`) |
| `CreatePipe(&mut r, &mut w, &sa, 0)` / `OwnedHandle::from_raw_handle` | out-pointers live; handles owned only after success |
| `(api.create)(..)` | both host handles alive for the call; `hpc` a live out-pointer |
| `mem::zeroed::<STARTUPINFOEXW / PROCESS_INFORMATION>()` | plain C structs; all-zero is valid |
| `Initialize/Update/DeleteProcThreadAttributeList` | buffer exactly the size asked for; deleted once, before the buffer frees; `hpc` outlives `CreateProcessW` |
| `CreateProcessW(..)` | command line writable and NUL-terminated; env block double-NUL-terminated UTF-16 with the Unicode flag; cwd null or NUL-terminated; all alive for the call |
| `CloseHandle(pi.hThread)`, `from_raw_handle(pi.hProcess)` | only after success; each handle closed or owned exactly once |
| `(api.resize)`, `(api.close)` | `hpc` live until `Conpty::drop`; `close` called exactly once there |
| `ReadFile` / `WriteFile` | slice pointer and length valid for the call (length clamped to `u32`); handle owned by the calling thread's closure |
| `RegisterWaitForSingleObject(.., Arc::as_ptr(&sender), ..)` | the `Arc` is held by the watcher until after `UnregisterWaitEx(.., INVALID_HANDLE_VALUE)` returns, which waits for a running callback; if that call fails, the `Arc` is leaked (`mem::forget` of a clone) so a late callback cannot dangle |
| callback `&*(context as *const ChildExitSender)` | as above: pointee alive for the whole callback |
| `GetExitCodeProcess`, `GetProcessId`, `WaitForSingleObject`, `TerminateProcess` | process handle owned by the watcher, closed only after `UnregisterWaitEx` and the grace wait |

## Ownership and drop order

| Object | Owner | Closed by | Order rule |
|---|---|---|---|
| Unix master fd | `PseudoConsole.master` | drop | hangs up the session (`SIGHUP`) |
| Unix slave fd | `spawn` local | end of `spawn` | after `Command` spawned the child |
| Unix `Child` | reaper thread | thread end | outlives the console; reaps the pid |
| conout/conin host ends | `spawn` locals | right after `CreatePseudoConsole` | before the child starts |
| `HPCON` | `Conpty` | `ClosePseudoConsole` in `Drop` | after the reader is in discard mode (**D3**) |
| conout / conin read/write ends | pipe threads | thread end | threads not joined |
| process handle | `ChildExitWatcher` | `OwnedHandle` drop | after `UnregisterWaitEx` and the grace wait |
| attribute list | `ProcThreadAttributeList` | `Drop` | after `CreateProcessW` |

Threads never call embedder code and hold only what they were given; none is joined.
