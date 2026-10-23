# oneterm-vt

An embeddable terminal core in Rust: VT parser, grid with scrollback, reflow, selection,
damage-tracked snapshots, key and mouse encoders, Sixel, and an optional pseudo-console.
You bring the pixels.

**Status:** under construction; see [docs/roadmap.md](docs/roadmap.md).

## Use

```toml
[dependencies]
oneterm-vt = { git = "<your fork url>", tag = "v0.1.0" }
```

Guide: [docs/user/](docs/user/README.md). Semver promise: [docs/user/versioning.md](docs/user/versioning.md).

## Layout

| Path | What |
|---|---|
| `crates/vt` | the library, package `oneterm-vt` |
| `crates/vt-bench` | `vt-bench`: benchmark tiers and the perf-history chart (not shipped) |
| `docs/user/` | embedder guide |
| `docs/perf/`, `perf/` | how performance is recorded; per-release records and chart |
| `docs/roadmap.md`, `docs/specs/` | build plan and per-phase specs |
| `scripts/release.sh` | bump, benchmark, chart, commit, tag |

## Developing

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

Git worktrees live under `.claude/worktrees/` and share the root `target/`. Rules, agents and
specs: `CLAUDE.md` and [docs/README.md](docs/README.md).

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
