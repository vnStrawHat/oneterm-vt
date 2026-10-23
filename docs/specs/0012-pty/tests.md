# Tests, CI check, user chapter

Real-shell tests run in plain `cargo test --workspace` on all three CI OSes (`pty` is default on).
Timeouts 10 s (15 s on Windows) per wait; liveness is checked by pid only, never by image name.

## Unit tests (in-crate)

| File | Tests (one behaviour each) |
|---|---|
| `pty.rs` | `env_validation` (empty key, `=` in key, NUL in key or value -> `InvalidInput`; normal entries pass); `pseudo_console_is_send` |
| `pty/loopback_tests.rs` | a loopback fake (two TCP socket pairs: I/O and child signal, an `mpsc` for events, a `Vec<WindowSize>` log) implementing all three traits: `loopback_implements_the_evented_contract` (output on the RW token, input reaches the "child", resize recorded, exit on the child token exactly once); `an_exit_without_a_code_is_still_an_exit` |
| `pty/unix.rs` | `identity_env_is_dropped_and_custom_wins` (`get_envs` on a `Command`); `master_and_slave_are_cloexec` (**D1**, `F_GETFD`); `winsize_pixels_saturate` (pure `winsize(size)`); `signal_mask_current_succeeds`; real `/bin/sh`: `output_reaches_the_reader` (`printf hi`), `input_reaches_the_child` (`/bin/cat`, `ping\n`), `exit_code_is_reported_once` (`exit 3`), `child_pid_is_positive`, `resize_of_a_live_pty_succeeds`, `rejected_resize_is_an_error` (0x0: `Err` names `TIOCSWINSZ` or `Ok`, never a panic) |
| `pty/windows.rs` | `escaping_table` (windows.md table); `escapes_only_when_asked` (`echo hello world` vs `echo "hello world"`); `default_shell_is_powershell`; `program_quoted_only_with_escape_args` (**D5**: `C:\Program Files\x.exe` + `a` -> `"C:\Program Files\x.exe" a` / verbatim); `coord_rejects_over_i16` (**D6**: 32767 ok, 32768 `InvalidInput`) |
| `pty/windows/conpty.rs` | `glyph_width_flags` (0x10, 0x08, default 0x10); env block: `empty_block_is_double_nul`, `parent_is_copied`, `case_insensitive_dedup_custom_wins`, `identity_names_dropped_custom_kept`, `block_is_sorted_case_insensitively` (**D4**: custom `b`, `A`, parent `=C:` -> `=C:=..`, `A=..`, `b=..`); `falls_back_to_system_without_dll` (empty temp dir) |
| `pty/windows/child.rs` | with `cmd.exe /c exit N` children: `exit_reported_once_with_code`; `instant_exit_not_missed`; `exit_before_registration_still_wakes`; `exit_queued_before_recorded` (30 children); `one_wake_per_registration` (re-register posts nothing; after `deregister` a new registration is owed one); `pid_available`; `deregistered_exit_still_observable` |
| `pty/windows/pipe_tests.rs` | over `std::io::pipe()`, ring 4096: `reader_with_bytes_left_is_woken_again`; `wakes_posted_before_a_poll_all_arrive` (priming + 2); `writer_with_room_is_woken_again`; `empty_reader_would_block` and `reader_sees_eof_after_pipe_closes` (Ok(0) + a wake, **D2**); `full_writer_would_block` (**D2**); `dropped_reader_keeps_draining` (**D3**: a thread writes 2x the ring into the pipe, nobody reads, drop the reader: the writes finish within 5 s) |
| `pty/windows/pseudo_console_tests.rs` | real `cmd.exe`: `output_arrives` (`/c echo vt-pty-ok`), `typed_input_echoes` (`/q /k echo off`, `echo vt-pty-typed\r\n`), `exit_code_reported` (`/c exit 3`), `drop_with_unread_output_finishes` (`/c dir /s C:\Windows\System32\drivers`, drop on a thread, done within 20 s; guards hosts before build 26100) |

The bundled-host branch of `ConptyApi` has no CI test: the crate ships no `conpty.dll`.

## Contract tests: `tests/pty_contract.rs` (public API only, all three OSes)

`#![cfg(feature = "pty")]`. Helpers: `script(cmd) -> Options` (Unix `/bin/sh -c cmd`; Windows
`cmd.exe /d /c cmd`) built with `..Options::default()` (proves no cfg field must be named);
`Session { pty, poller: Arc<Poller>, events }` registering `Event::readable(0)`, `Level`;
`pump(until, timeout)`: `wait(100 ms)`, on the RW token read until `WouldBlock`/end (api.md
table), on the child token keep the event; deregisters in `Drop`. `alive(pid)`: Unix
`kill -0 <pid>` via `Command`; Windows `tasklist /FI "PID eq <pid>" /NH /FO CSV`.

| Test | Assert |
|---|---|
| `echo_round_trip` | `echo vt-pty-contract` -> marker arrives on the RW token |
| `typed_input_reaches_the_child` | interactive child (Unix `/bin/cat`, Windows `cmd /q /k echo off`); write a line; the marker echoes |
| `exit_code_arrives_on_child_token` | `exit 3` -> `Exited(Some(s))`, `s.code() == Some(3)`, from a child-token wake without reading; second call `None` |
| `env_reaches_the_child` | `env {VT_PTY_PROBE: "probe-42"}`; print it (`$VT_PTY_PROBE` / `%VT_PTY_PROBE%`) -> `probe-42` |
| `cwd_applies` | temp dir `vt-pty-cwd-<pid>`; print cwd (`pwd` / `cd`) -> the dir name appears |
| `resize_on_both_sides_of_exit` | live resize `Ok`; after `Exited`, resize is `Ok` or `Err`, no panic |
| `child_sees_the_new_size` (Unix) | `read x; stty size`; resize 30x100; write `\n` -> `30 100` |
| `drop_ends_the_child` | Unix `sleep 30`: dead within 5 s of drop; Windows `ping -n 30 127.0.0.1` after first output: drop < 4 s, then dead |
| `drop_ends_only_its_own_child` (Unix, **D1**) | consoles A and B (`sleep 30`); drop A -> A dead within 5 s, B alive; then drop B |

## CI check (`.github/workflows/ci.yml`, step "Without default features", Linux)

```bash
deps="$(cargo tree -p oneterm-vt -e normal --no-default-features --prefix none --format '{p}' \
  | awk 'NR > 1 { print $1 }' | sort -u | tr '\n' ' ')"
[ "$deps" = "bitflags log memchr rustc-hash unicode-segmentation unicode-width " ] \
  || { echo "::error::--no-default-features dependency set changed: $deps"; exit 1; }
```

## `docs/user/pty.md` (chapter 10, <= 120 lines)

1. Example first: the poll loop, a ` ```rust,no_run ` doctest (compiled, never run): visible
   `#![allow(unsafe_code)]`; `Options` with `TERM=xterm-256color` in `env`; `spawn`; `Poller`;
   `unsafe { register(.., Event::readable(PTY_READ_WRITE_TOKEN), Level) }`; loop on `wait`:
   RW token -> read per the table into `Terminal::feed`; child token -> drain once more,
   `deregister`, return the status.
2. What you owe (checklist): `polling = "3"` in your manifest; deregister before dropping the
   poller; read until `WouldBlock`; write `VtEvent::Reply` bytes and encoded keys to `writer()`,
   keeping short writes; `OnResize::on_resize` with the numbers given to `Terminal::resize`;
   `TERM`/`COLORTERM` in `Options::env`; drop on an owner thread.
3. Reader/writer return-value table (api.md).
4. Platform table: default shell, `escape_args`, `child_signal_mask`/`SignalMask`, cwd rule,
   pixel fields (Unix only), drop behaviour (Windows blocks up to ~2 s; Unix hangs up, never
   signals), crate threads (Windows 2, Unix 1; none joined).
5. Windows console host: `conpty.dll` + `x64\OpenConsole.exe` beside your executable, a
   matched pair; without them Sixel does not survive the inbox host.
6. Turning `pty` off: `default-features = false`; the six leaf dependencies; `feed` is the seam.

Wired in `lib.rs` as `#[cfg(all(doctest, feature = "pty"))] #[doc = include_str!(..)] mod
user_guide_pty {}`; row 10 of `docs/user/README.md` becomes `ready` with a link.
