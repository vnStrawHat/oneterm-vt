# Windows: `src/pty/windows.rs` + `windows/{conpty,child,pipe}.rs` (ConPTY)

```rust
pub struct PseudoConsole {           // field order IS drop order (D3)
    conout: PipeReader,              // 1st: switches its pipe thread to drain-and-discard
    conpty: Conpty,                  // 2nd: ClosePseudoConsole while conout is still drained
    conin: PipeWriter,
    child: ChildExitWatcher,         // last: grace period, then TerminateProcess
}
```

## Host selection (`conpty.rs`, `ConptyApi::resolve`, every spawn)

1. `current_exe()?.parent()/conpty.dll`: `LoadLibraryW(absolute path)`; never freed (the fn
   pointers live for the process). `GetProcAddress` for `CreatePseudoConsole`,
   `ResizePseudoConsole`, `ClosePseudoConsole`; transmute each to its kernel32 signature.
2. Missing file -> inbox API (`windows_sys` kernel32 imports). Loaded but an export missing ->
   `log::warn!` naming the path and export, then the inbox API.
3. `log::info!("conpty: bundled" | "conpty: system")` once per process (`Once`).
- The order is load-bearing: the inbox `conhost.exe` does not pass Sixel DCS through; a matched
  `conpty.dll` + `x64\OpenConsole.exe` beside the embedder's executable does. The crate ships
  neither file (docs/user/pty.md says how to place them). Minimum host: Windows 10 1809.

## `spawn(options, size)`, in order

| # | Step |
|---|---|
| 1 | validate env (**D4**); `coord(size)?`: rows/cols above `i16::MAX` -> `InvalidInput` (**D6**) |
| 2 | `ConptyApi::resolve()` |
| 3 | `CreatePipe` x2, `SECURITY_ATTRIBUTES { bInheritHandle: 0 }`, default size: `(conout, conout_host)`, `(conin_host, conin)`; all four `OwnedHandle` |
| 4 | `create(coord, conin_host, conout_host, glyph_flag, &mut hpc)`; `!= S_OK` -> error with HRESULT. Flags: `WcsWidth` = `0x10`, `Graphemes` = `0x08` |
| 5 | drop `conin_host`, `conout_host` (the host duplicated them; our copies would block EOF) |
| 6 | `Conpty { handle, api }` owned now, so every later `?` closes the console |
| 7 | `STARTUPINFOEXW` zeroed, `cb = size_of::<STARTUPINFOEXW>()`, `dwFlags |= STARTF_USESTDHANDLES` with null std handles (the child inherits none of ours) |
| 8 | attribute list: `InitializeProcThreadAttributeList(null, 1, 0, &mut size)` (expected to fail and set `size`; success is an error), `Box<[u8]>` of `size`, initialise again, `UpdateProcThreadAttribute(list, 0, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, hpc as *mut c_void, size_of::<HPCON>(), null, null)`; RAII `DeleteProcThreadAttributeList` |
| 9 | command line (below) as a mutable NUL-terminated `Vec<u16>`; cwd as `Vec<u16>` or null; env block (below) |
| 10 | `CreateProcessW(null, cmd, null, null, FALSE, EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT, env, cwd, &startup.StartupInfo, &mut pi)`; `0` -> `cannot start ..` |
| 11 | `CloseHandle(pi.hThread)`; `OwnedHandle::from_raw_handle(pi.hProcess)` |
| 12 | build `ChildExitWatcher::new(process)`, `PipeWriter::new(conin, 1 MiB)`, then `PipeReader::new(conout, 1 MiB)` as locals in the reverse of the field drop order (a `?` inside a struct literal would drop in the wrong order; the watcher first so its drop gives the child a grace period), then the struct |

## Command line (`cmdline`, `push_escaped_arg`)

`program`, then for each arg `' '` + (escaped if `escape_args`, else verbatim). Default program
`powershell`. With `escape_args` the program is wrapped in `"` when it contains a space or tab and
no `"` (**D5**; the program token ends at the next quote, no backslash escapes). Arg escaping:

- quote iff the arg is empty or contains `' '` or `'\t'`;
- walk chars keeping a run count of `\`; every char is emitted as is, except that before a `"`
  the run is extended to `2n+1` backslashes (emit `n+1` more); any non-`\` resets the run;
- when quoted, after the last char emit `n` more backslashes (trailing run doubled), then `"`.

Table (input -> output): `abc`->`abc`; empty->`""`; ` `->`" "`; `ab c`->`"ab c"`; `ab\tc`->`"ab\tc"`;
`ab\c`->`ab\c`; `ab"c`->`ab\"c`; `"`->`\"`; `a"b"c`->`a\"b\"c`; `ab "c`->`"ab \"c"`;
`a "b" c`->`"a \"b\" c"`; `C:\Program Files\`->`"C:\Program Files\\"`;
`C:\Program Files\a.txt`->`"C:\Program Files\a.txt"`.

## Environment block (`environment_block(custom, parent)`, **D4**)

`BTreeMap<Vec<u16> /* ASCII-uppercased key */, (OsString, OsString)>`: insert `custom` sorted by
key (first wins on a case-insensitive clash, later ones `log::warn!`); reserve the dropped names
not set by `custom`; insert `std::env::vars_os()` entries whose folded key is free (this keeps
the hidden `=C:` drive entries). Emit `key=value\0` in map order, then `\0`; empty -> `\0\0`.
Always a block, never null (null would copy the parent with the dropped names).
`// ponytail: ASCII case folding; non-ASCII names sort by code unit.`

## Pipes made pollable (`pipe.rs`)

Anonymous pipes cannot join `polling`'s IOCP, so each direction has one thread doing blocking
`ReadFile`/`WriteFile` and a bounded ring (`Mutex<VecDeque<u8>>` + `Condvar`, 1 MiB, 64 KiB chunks);
the ring posts `CompletionPacket`s to the registered poller (`PollerIocpExt::post`).

| Rule | Detail |
|---|---|
| interest | `Option<{ event, packet (built once, cloned per post), Arc<Poller>, mode }>` |
| wake | post if registered and the side's flag (`readable` / `writable`) is set; `Oneshot`/`EdgeOneshot` clear the interest after posting |
| level emulation | `read` leaving bytes and `write` leaving room post again; an empty reader / full writer sets `caller_waiting`, and the pipe thread posts when it changes that |
| priming | the reader's first `register` posts once even when empty |
| reader `read` | copy out; empty and open -> `WouldBlock`; empty and `eof` -> `Ok(0)` (**D2**) |
| writer `write` | take what fits; no room -> `WouldBlock` (**D2**); `flush` is a no-op |
| conout thread end | `ReadFile` error or 0 -> `eof = true`, wake, return (**D2**) |
| reader drop | `closed = true`, notify; the thread keeps reading and discarding until the pipe breaks (**D3**) |
| writer drop | `closed = true`, notify; the thread returns after the current write |
| threads | `oneterm-vt-pty-conout`, `oneterm-vt-pty-conin`; never joined; spawn failure fails `spawn` |
| poison | `lock()` recovers `PoisonError::into_inner` (bytes stay valid) |

## Child exit (`child.rs`, `ChildExitWatcher`) (decision: wait on the process handle)

- `RegisterWaitForSingleObject(&mut wait, process, child_exited, Arc::as_ptr(&sender), INFINITE,
  WT_EXECUTEINWAITTHREAD | WT_EXECUTEONLYONCE)`; it fires at once for an already-exited child.
- Callback: `GetExitCodeProcess` -> `Exited(Some(ExitStatus::from_raw(code)))` or `Exited(None)`;
  `send` first, then under the `notify` lock `exited = true` and post if registered (`posted = true`).
- `register` (records `{poller, event}`; key child token) posts at once if `exited && !posted`;
  `deregister` clears the interest and `posted`. One wake per registration: re-registering after
  the wake posts nothing (a loop that re-registers to toggle write interest must not spin).
- No job object: only the direct child is ever touched, through its own handle.
- Drop: `UnregisterWaitEx(wait, INVALID_HANDLE_VALUE)` (waits for a running callback), then
  `WaitForSingleObject(process, 2000)`; not signalled -> `log::warn!` + `TerminateProcess(process, 1)`
  (failure logged). Grace 2 s: a started client exits within milliseconds of the close; one still
  initialising may never handle the close request.

## Registration, resize, drop

- `register` = `reregister`: `conin` and `conout` record `interest` with key RW token; `child`
  records it with the child key. `deregister` clears all three. All return `Ok(())`.
- `on_resize`: `coord(size)?` then `(api.resize)(hpc, coord)`; `!= S_OK` -> error. Pixel fields unused.
- Drop (field order): reader to discard mode; `ClosePseudoConsole` (returns at once from build
  26100; earlier hosts wait until the host exits, which needs conout drained); writer closed;
  watcher grace/terminate. The drop blocks up to about 2 s: owner thread, not UI thread (rustdoc).
- On hosts before build 26100, `drop` can block longer: `ClosePseudoConsole` waits for a client
  that never disconnects. Upgrade path, if it ever matters: close the console on a detached
  thread while the grace/terminate step runs.
