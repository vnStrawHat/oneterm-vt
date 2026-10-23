# Examples: `headless.rs` and the README quick start

## `crates/vt/examples/headless.rs`

A terminal with no window: feed one buffer, print the events, print the screen as text.
Engine items only (no `pty`), so it also builds with `--no-default-features`.

| Part | Content |
|---|---|
| module doc | one paragraph: what it shows; `Run: cargo run -p oneterm-vt --example headless` |
| lint | `#![allow(clippy::print_stdout)]` with a `//` reason ("an example's whole job is printing"); the workspace warns on `print_stdout` and CI denies warnings |
| `INPUT` | one `const &[u8]`: OSC 0 title, OSC 7 `file://host/tmp`, a private OSC `7501;hello` the embedder routes to itself, `\x1b[1;32mhello\x1b[0m world`, `\x1b[3;3Hrow three`, DA1 `\x1b[c` |
| config | `OscRoutes::new()` then `.route(7501, OscRoute::Forward)`; `Config { osc_routes, product_name: Some("headless-example(0.1.0)".into()), ..Config::default() }` |
| feed | `Terminal::new(Size { rows: 4, cols: 32 }, config)`; one `EventBatch`; `let stats = term.feed(INPUT, &mut batch, Instant::now())` |
| events | `-- events --`, one line per event: `Title` -> `title  "<text>"`; `Cwd` -> host and path; `Osc { code, params, .. }` -> code and the params after the number joined by `;`; `Reply` -> `reply  <bytes.escape_ascii()>` with "write these to the program" comment; any other -> `{:?}` |
| stats | `"{} bytes fed, {} unhandled sequences"` from `FeedStats` |
| screen | one reused `SnapshotState`, `snapshot_update`, `-- screen (<update:?>) --`, then each row as `\|<text>\|` |
| `row_text` | private fn: a snapshot row as plain text; skips wide-glyph spacer cells, expands clusters (`SnapshotRow::cluster`) |

- No `unwrap`/`expect`; `main` returns `()`. No new dependency, no `[[example]]` table needed.
- Comments say why (spans index their batch; the state is reused so the next update is a diff).
- `row_text` is the same helper `for-agents.md` and `quickstart.md` use; it is copied, not made
  public (Decision 7). Three short copies beat a new public item outside the roadmap lists.

CI runs it on all three OS (files.md): proves it builds and runs to the end without a panic.
Its output is not asserted; the doctests assert the same behaviour.

Expected output shape (exact event lines depend on the final `VtEvent` set):

```text
-- events --
title      "headless example"
cwd        host "host", path "/tmp"
osc 7501   "hello"
reply      \x1b[?62;4;22c
Repaint
<n> bytes fed, 0 unhandled sequences
-- screen (Full) --
|hello world                     |
|                                |
|  row three                     |
|                                |
```

## Root `README.md`: quick start, compiled

New section `## Quick start` after "Use", a shortened copy of the `quickstart.md` example
(at most 20 lines): `Terminal::new`, `feed` with text and a DA1 query, replies into a `Vec`,
one `SnapshotState` and `snapshot_update`, row 0 text asserted. Uses `assert_eq!`, prints nothing.
One line under it: "Next: [docs/user/](docs/user/README.md); AI agents start at
[for-agents.md](docs/user/for-agents.md)."

Wiring in `crates/vt/src/lib.rs`, beside the chapter modules:

```rust
#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
mod readme {}
```

Other README blocks are tagged (`toml`, `bash`), so rustdoc skips them.

Other README edits (files.md): status line, layout rows for `fuzz/` and `scripts/public-api.sh`,
"Developing" gains `scripts/public-api.sh check`.
