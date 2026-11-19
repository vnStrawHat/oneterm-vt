# Pseudo-console: platforms

What differs between Unix and Windows behind [`pty::PseudoConsole`](pty.md), the Windows console host,
and building without the `pty` feature.

## Unix and Windows

| | Unix | Windows (10 1809+) |
|---|---|---|
| default shell | `$SHELL`, else `/bin/sh` | `cmd.exe`, by its full path under `%SystemRoot%` |
| arguments | passed to `exec` as they are | one command line; `Options::escape_args` (default `false`) quotes them; a program path with a space is always quoted |
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
