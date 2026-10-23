# User docs: final chapter list and what phase 13 writes

## Final list (`docs/user/`)

| # | File | Written in phase | Phase 13 action |
|---|---|---|---|
| - | `README.md` | 0 | rewrite the top (below); every row linked; drop the Phase and Status columns |
| 00 | `for-agents.md` | 13 | **write** (below) |
| 01 | `quickstart.md` | 7 | check (below) |
| 02-10 | `threading`, `events`, `input`, `search`, `graphics`, `resize`, `limits`, `conformance`, `pty` | 4-11 | none |
| 11 | `versioning.md` | 0 | update (below) |
| 12 | `performance.md` | 12 | one line (below) |
| 13 | `api-map.md` | 13 | **write** (below) |
| 14-18 | `parser`, `cells-and-styles`, `grid`, `selection`, `snapshot` | 1-7 | none |

If a chapter of phases 7-12 is missing, stop and report: phase 13 does not write it. A chapter
whose example no longer matches the final API is fixed here (that is a stale doctest, a bug).

## Rules for every file

- At most ~120 lines; `api-map.md` at most 170 (a lookup table, not prose).
- Example first, then rules as bullets or tables. Clear and short; for developers and AI agents.
- Items named by path below `oneterm_vt::` (`input::encode_key`); no private module names.
- Every Rust block compiles: ` ```rust `, or ` ```rust,no_run ` where it would spawn a process
  (`pty.md`); never `ignore`. Every other block carries a tag (`text`, `toml`, `bash`, `json`).
- Every `docs/user/*.md` is wired into `crates/vt/src/lib.rs` as a `#[cfg(doctest)]` module with
  `include_str!`, also files without Rust blocks (one uniform rule; files.md lists the modules).
  `tests/user_guide.rs` fails when a file is not wired (tests.md T1).
- Links: relative between chapters; no link into `docs/specs/`; no mention of other projects.

## `README.md` (index)

1. Title and one sentence: who it is for.
2. `## Read this first` replaces "The crate in five lines"; exactly these ten numbered lines:
   1. AI agent? Read [`for-agents.md`](for-agents.md) first: the loop and five common mistakes.
   2. Looking for a name? [`api-map.md`](api-map.md) lists every public type, one line each.
   3. Every Rust block in this guide compiles against this version; copy them.
   4. You own the transport, the lock, the threads; the crate owns bytes-to-grid semantics.
   5. `Terminal::feed(bytes, &mut batch, now)` never fails and never panics; drops are counted in `FeedStats`.
   6. Write every `VtEvent::Reply` back to the program, in order.
   7. Draw from a reused `SnapshotState`, never from the grid under your lock.
   8. Turn keys into bytes with `Terminal::encode_key`; it follows the program's modes.
   9. `pty` is a default feature; `default-features = false` leaves the engine alone.
   10. What is promised across versions: [`versioning.md`](versioning.md).
3. Status line: "v0.1.0. Every chapter is written; every Rust block compiles in `cargo test --doc`."
4. Chapters table: `#`, `Chapter` (link), `Covers`; rows 00-18 in number order.
5. Features table unchanged. "Writing rules" kept, plus: "`tests/user_guide.rs` checks the wiring."

## `quickstart.md` check

Confirm, fix if not: its first block builds a `Terminal`, feeds bytes containing a query,
writes `Reply` bytes back, calls `snapshot_update` and reads one row as text; it ends with a
"Next" line linking `for-agents.md`, `threading.md` and `api-map.md`.

## `versioning.md` update (stays under 120 lines)

- Pin example: `oneterm-vt = { git = "<repository url>", tag = "v0.1.0" }` (open item 1).
- "Before 1.0": add "a minor or patch change goes `0.x.y` to `0.x.(y+1)`; `scripts/release.sh`
  applies this mapping".
- MSRV: "1.96.0 (`rust-version`); raising it is a major change."
- New section "The public-API snapshot": the two files and the table "snapshot diff -> kind"
  from public-api.md; "behaviour-only rules above (reply bytes, `FeedStats`) are not visible in
  the snapshot and still apply".

## `performance.md`

One line under the numbers table: "Each release's own run is `perf/history/v<version>.json`;
`perf/chart.svg` compares releases." The table stays the phase-12 run (Decision 8).

## `api-map.md` (row 13)

```markdown
# API map

Every public type and trait of `oneterm-vt` 0.1.0, one line each, grouped by what you do with
it. Paths are below `oneterm_vt::`. The last row of a group lists its functions and constants.

## Terminal and configuration

| Item | What it is for | Chapter |
|---|---|---|
| `Terminal` | the engine: `feed` bytes, `resize`, `snapshot_update`, `encode_key` | [quickstart](quickstart.md) |
```

- Groups, in order: Terminal and configuration; Events; Snapshot and drawing; Cells and styles;
  Positions, rows and resize; Selection; Graphics; `parser`; `intern`; `grid`; `search`
  (`regex` feature noted on its row); `input`; `pty` (feature `pty`; rows marked "Unix only" /
  "Windows only").
- One row per type or trait, from the snapshot files (public-api.md). "What it is for": at
  most ~12 words, the use, not the implementation. Chapter: the chapter that teaches it.
- Coverage is enforced: tests.md T2 fails when a type in either snapshot has no row.

## `for-agents.md` (row 00, at most 100 lines)

1. Title, then three lines: the crate turns program output into a grid and events, and keys
   into program input; it never does I/O; the loop below is the whole contract.
2. `## The loop`, one doctest, no `pty`: `Terminal::new(Size { rows: 4, cols: 20 }, ..)`;
   `feed(b"hello\r\n\x1b[?1h\x1b[c", ..)` (text, application cursor keys, DA1); collect
   `VtEvent::Reply` bytes into `to_program` and assert it is the DA1 answer; one reused
   `SnapshotState`, `snapshot_update`, row 0 as text == `"hello"`;
   `term.encode_key(&input::KeySpec::Named(input::NamedKey::ArrowUp), input::KeyMods::default())`
   == `Some(b"\x1bOA".to_vec())` (DECCKM is on). Comments number the four steps.
3. `## What you owe`: checklist: replies back in order; spans read before the next `feed`; one
   `SnapshotState` per view; resize the terminal and the transport together; keys through the
   encoders; clipboard, title and notifications are your policy.
4. `## Five common mistakes`: table Mistake | Symptom | Do instead:

| Mistake | Symptom | Do instead |
|---|---|---|
| dropping `VtEvent::Reply` | programs stall or time out waiting for DA1, DSR, cursor reports | write `batch.bytes(span)` to the program right after `feed`, in order |
| keeping a span past the next `feed` | wrong or empty text; spans index *their* batch | resolve with `batch.str` / `bytes` / `params` first; copy what you keep |
| a new `SnapshotState` per frame | every update is `Full`; damage tracking lost | one state per view; on `Partial { scrolled }` shift, then redraw `changed()` |
| hand-written key bytes (`"\x1b[A"`) | arrows break in full-screen apps and kitty-keyboard apps | `Terminal::encode_key` / `encode_key_event`; send nothing on `None` |
| resizing one side only | the shell wraps at the old width; prompt garbled | `Terminal::resize` and the transport's resize with the same `Size` |

5. `## Testing a program headlessly`: feed recorded bytes, assert on snapshot text, on reply
   bytes and on `FeedStats` counters; no window, no process needed.
6. `## Where next`: task -> chapter table, at most 8 rows (threading, events, input, snapshot,
   pty, limits, api-map, versioning).
