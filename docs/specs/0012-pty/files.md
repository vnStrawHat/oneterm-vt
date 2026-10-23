# Files, dependencies, CI, docs

Builds on specs 0001-0011 as implemented. Sub-milestones, gate green after each:
**11a** manifests, `pty.rs` shared items, loopback tests, tree check; **11b** Unix;
**11c** Windows; **11d** contract tests, user chapter, versioning line.

## `crates/vt/src` (new)

| Path | Content | cfg |
|---|---|---|
| `pty.rs` | module doc (both platforms in prose, api.md rustdoc rules); tokens, `GlyphWidth`, `Shell`, `Options`, `WindowSize`, `ChildEvent`, the three traits, `DROPPED_PARENT_ENV`, `validate_env`; `mod unix` / `mod windows` + re-exports; `#[cfg(test)] mod loopback_tests;` | `feature = "pty"` (from `lib.rs`) |
| `pty/unix.rs` | `PseudoConsole`, `SignalMask`, `open_pty`, `set_cloexec`, `set_utf8_input`, `winsize`, `set_window_size`, `set_nonblocking`, `apply_env`, `default_shell`, `reap_in_background`; inline tests | `unix` |
| `pty/windows.rs` | `PseudoConsole` (field order), trait impls, `cmdline`, `push_escaped_arg`, `coord`, `win32_string`, `PIPE_CAPACITY`; `mod conpty; mod child; mod pipe;`; inline tests; `#[cfg(test)] mod pseudo_console_tests;` | `windows` |
| `pty/windows/conpty.rs` | `ConptyApi`, `ConptyBackend` (log name), `Conpty` (+ `Drop`), `spawn`, `anonymous_pipe`, `ProcThreadAttributeList`, `environment_block`, glyph flags; inline tests | |
| `pty/windows/child.rs` | `ChildExitWatcher`, `ChildExitSender`, `Notify`, `child_exited`, `CHILD_EXIT_GRACE`; inline tests | |
| `pty/windows/pipe.rs` | `Ring`, `PipeReader`, `PipeWriter`, `push`/`pull`, pipe threads; `#[cfg(test)] #[path = "pipe_tests.rs"] mod pipe_tests;` | |
| `pty/loopback_tests.rs`, `pty/windows/pipe_tests.rs`, `pty/windows/pseudo_console_tests.rs` | tests.md | |

`unix.rs` and `windows.rs` are declared with `#[cfg(unix)]` / `#[cfg(windows)]` on the `mod`
line; no other platform gating outside `pty/`.

## `crates/vt/src/lib.rs` (edits)

```rust
#[cfg(feature = "pty")]
pub mod pty;

#[cfg(all(doctest, feature = "pty"))]
#[doc = include_str!("../../../docs/user/pty.md")]
mod user_guide_pty {}
```

Crate doc gains one sentence: "With the default `pty` feature, [`pty`] starts a shell behind a
pseudo-console (ConPTY on Windows, `openpty` on Unix) that an embedder polls with `polling`."
(The link is fine: `pty` exists whenever docs build with default features; `cargo doc
--no-default-features` is not in CI.)

## Root `Cargo.toml`

`[workspace.dependencies]`: `polling = "3.11"` and `libc = "0.2.186"` unchanged;
`windows-sys = "0.61"` (**D7**; comment: "feature `pty`, Windows; the version polling 3.11 uses").

## `crates/vt/Cargo.toml`

```toml
[features]
default = ["pty"]
# Pseudo-console transport (ConPTY / openpty), pollable with `polling`.
pty = ["dep:polling", "dep:windows-sys", "dep:libc"]

[dependencies]
bitflags.workspace = true
log.workspace = true            # pty diagnostics; one of the six no-default leaf crates (roadmap)
memchr.workspace = true
# PUBLIC dependency under `pty`: Poller, Event and PollMode appear in EventedReadWrite.
polling = { workspace = true, optional = true }
rustc-hash.workspace = true
unicode-segmentation.workspace = true
unicode-width.workspace = true

[target.'cfg(windows)'.dependencies]
windows-sys = { workspace = true, optional = true, features = [
    "Win32_Foundation", "Win32_Security", "Win32_Storage_FileSystem", "Win32_System_Console",
    "Win32_System_IO", "Win32_System_LibraryLoader", "Win32_System_Pipes", "Win32_System_Threading",
] }

[target.'cfg(unix)'.dependencies]
libc = { workspace = true, optional = true }
```

`Win32_System_IO` is needed by the `ReadFile`/`WriteFile` signatures (`OVERLAPPED`),
`Win32_Security` by `CreatePipe` (`SECURITY_ATTRIBUTES`). Keep the existing `regex`/`hotpath`
entries as they are. No new dev-dependency (liveness checks shell out to `kill` / `tasklist`).

## CI (`.github/workflows/ci.yml`)

- "Without default features" step (Linux) gains the tree check from tests.md after the existing
  clippy and test lines.
- Nothing else: `cargo test --workspace` already runs the pty tests on all three OSes, and
  clippy `--all-features` covers the `pty` code on each OS.

## Docs

| Path | Change |
|---|---|
| `docs/user/pty.md` | new chapter (tests.md outline); row 10 of `docs/user/README.md` -> `ready` + link |
| `docs/user/versioning.md` | one bullet under Details: "`polling` is the one public dependency (under `pty`); a `polling` major bump is a `major` bump here." |
| `docs/user/README.md` Features row `pty` | unchanged (already names `polling` + `windows-sys` or `libc`) |

## Rules

- No `unsafe` outside the lifts in safety.md; no `unwrap`/`expect` outside tests; errors per api.md.
- Threads only inside `pty`; the engine stays thread-free without the feature.
- Rustdoc stands alone (no `docs/` paths). The `pty` module doc describes both platforms in prose
  (rustdoc renders only the host's half); platform-only items are named in backticks, never
  intra-doc linked, so `cargo doc -D warnings` passes on every OS.

## Commit

`feat(pty): pseudo-console over ConPTY and openpty, pollable with polling` (minor).
Body: `Spec: docs/specs/0012-pty`.
