# oneterm-vt

An embeddable terminal core in Rust: VT parser, grid with scrollback, reflow, selection,
damage-tracked snapshots, key and mouse encoders, Sixel, and an optional pseudo-console.
You bring the pixels.

**Status:** v0.1.0. Built and tested on Linux, Windows and macOS.

## Use

```toml
[dependencies]
oneterm-vt = { git = "<your fork url>", tag = "v0.1.0" }
```

Guide: [docs/user/](docs/user/README.md). Semver promise: [docs/user/versioning.md](docs/user/versioning.md).

## Quick start

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, Size, SnapshotContent, SnapshotState, Terminal, VtEvent};

let now = Instant::now();
let mut term = Terminal::new(Size { rows: 4, cols: 20 }, Config::default());
let (mut batch, mut state) = (EventBatch::new(), SnapshotState::new());

term.feed(b"hello world\r\n\x1b[c", &mut batch, now); // text, then a DA1 query
let mut to_program = Vec::new(); // write these back to the program, in order
for event in batch.iter() {
    if let VtEvent::Reply(span) = event {
        to_program.extend_from_slice(batch.bytes(*span));
    }
}
assert_eq!(to_program, b"\x1b[?62;4;22c");

term.snapshot_update(&mut state, now);
let row = &state.rows()[0];
let text: String = row.cells.iter().filter(|cell| !cell.width.is_spacer())
    .filter_map(|cell| match cell.content { SnapshotContent::Scalar(c) => Some(c), _ => None })
    .collect();
assert_eq!(text.trim_end(), "hello world");
```

Next: [docs/user/](docs/user/README.md); AI agents start at [for-agents.md](docs/user/for-agents.md).

## Layout

| Path | What |
|---|---|
| `crates/vt` | the library, package `oneterm-vt` |
| `crates/vt-bench` | `vt-bench`: benchmark tiers and the perf-history chart (not shipped) |
| `docs/user/` | embedder guide |
| `docs/perf/`, `perf/` | how performance is recorded; per-release records and chart |
| `docs/roadmap.md`, `docs/specs/` | build plan and per-phase specs |
| `fuzz/` | cargo-fuzz targets, run before each release (not a workspace member) |
| `scripts/public-api.sh` | check or update the public-API snapshot |
| `scripts/release.sh` | bump, gate, benchmark, chart, commit, tag; run only by the Release workflow |
| `.github/workflows/` | CI on 3 OS; the Release workflow (manual) |

## Developing

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
scripts/public-api.sh check   # needs rustup; installs cargo-public-api and its pinned nightly
```

Git worktrees live under `.claude/worktrees/` and share the root `target/`. Rules, agents and
specs: `CLAUDE.md` and [docs/README.md](docs/README.md).

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
