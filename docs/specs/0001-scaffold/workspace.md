# Workspace and crates

## Remove

| Path | Why |
|---|---|
| `crates/app/` | Template placeholder. |
| `tools/` (only `.gitkeep`) | Template leftover; scripts live in `scripts/`. |
| `tracing`, `tracing-subscriber`, `thiserror` in `[workspace.dependencies]` | Unused. The engine logs with `log` and returns no domain errors (hostile input is degraded and counted; `pty` returns `std::io::Error`). Re-add `thiserror` if a later spec needs it. |

## Root `Cargo.toml`

Keep `[workspace]`, `[workspace.package]`, `[workspace.lints.*]` and every `[profile.*]` unchanged.
Replace `[workspace.dependencies]` with the table below. Versions are the minimum tested
versions. A crate adds an entry (`x.workspace = true`) only in the phase that first uses it.

```toml
[workspace.dependencies]
# vt-bench (phase 0)
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1.0.150"
# oneterm-vt, added per phase (see docs/roadmap.md)
bitflags = "2.13"
log = "0.4.34"
memchr = "2.8"
rustc-hash = "2.1"
unicode-segmentation = "1.13.3"
unicode-width = "0.2.2"            # width parity depends on this exact table version
regex = "1.12"                     # feature `regex`, phase 8
hotpath = "0.26"                   # feature `hotpath-profiling`, phase 12
polling = "3.11"                   # feature `pty`, phase 11 (public dependency)
libc = "0.2.186"                   # feature `pty`, Unix, phase 11
windows-sys = "0.59"               # feature `pty`, Windows, phase 11 picks the API features
proptest = "1.11"                  # dev-dependency from phase 1
```

## `crates/vt/Cargo.toml`

```toml
[package]
name = "oneterm-vt"
description = "An embeddable terminal core: VT parser, grid with scrollback, reflow, selection, damage-tracked snapshots, Sixel and an optional pseudo-console transport."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true

[features]
default = ["pty"]
# Pseudo-console transport (ConPTY / openpty). Gains its dependencies in phase 11.
pty = []
# Whole-history integrity walk after every mutation; for tests, fuzzing and CI only.
vt-paranoid = []
# Regular-expression scrollback search. Gains `dep:regex` in phase 8.
regex = []
# `hotpath::measure` sites on hot paths. Gains `dep:hotpath` in phase 12.
hotpath-profiling = []

[lints]
workspace = true
```

No `[dependencies]` yet. Features stay empty until their phase; the names are fixed now so CI's
`--all-features` and `--no-default-features` runs are meaningful from day one.

## `crates/vt/src/lib.rs` (the only source file)

```rust
//! `oneterm-vt`: an embeddable terminal core.
//!
//! Bytes in; a grid, events and damage-tracked snapshots out. No rendering, no policy,
//! no lock, no global state. Terminal input is untrusted: nothing here panics on it or
//! returns an error for it. Malformed input is dropped, truncated or degraded, and counted.
//!
//! The engine is being rebuilt module by module; this release has no public items yet.

#![warn(missing_docs)]
```

Rustdoc must stand alone: no `docs/...` or `crates/...` paths in `//!` or `///` text.

## `crates/vt-bench/Cargo.toml`

```toml
[package]
name = "oneterm-vt-bench"
description = "Benchmark tiers and the perf-history chart for oneterm-vt. Developer tool, not shipped."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true

[[bin]]
name = "vt-bench"
path = "src/main.rs"

[dependencies]
oneterm-vt = { path = "../vt" }
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true

[lints]
workspace = true
```

Files: `src/main.rs`, `src/perf_history.rs`, `src/chart.rs`; see [bench-cli.md](bench-cli.md).
`vt-bench` reads `CARGO_PKG_VERSION` as the release version: it shares the workspace version.

## License files (repo root)

| File | Content |
|---|---|
| `LICENSE` | Apache License 2.0, verbatim from <https://www.apache.org/licenses/LICENSE-2.0.txt>. |
| `NOTICE` | Text in [docs-text.md](docs-text.md). |
