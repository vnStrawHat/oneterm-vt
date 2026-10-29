# Cross-engine comparison

oneterm-vt against eight other embeddable VT engines on one machine: the same 23 fixtures, the
same knobs, one process per run, and no number without a verified end state. Spec:
`docs/specs/0016-cross-engine-bench/`. This folder is outside the root workspace and CI; heavy
dependencies (wezterm, Zig, Node) stay away from the crates.

Layer: grid (parse + apply to terminal state, 64 KiB chunks, 160x45, 10 000 history lines) and
memory. No rendering, no PTY. Results are not quoted in the root README or `docs/user/`.

## Prerequisites

Git Bash (GNU coreutils, `curl`, `unzip`), Rust 1.96 with MSVC Build Tools, Node 22. Zig is
fetched. Network is needed by `fetch-engines.sh` and `compare build` only.

## Commands

```bash
bash compare/fetch-engines.sh          # sources and libghostty-vt, pinned (once)
cargo build --release --manifest-path compare/Cargo.toml -p compare-driver
target/release/compare.exe <command>   # `compare` on Linux; run it directly, not via `cargo run`
```

`run` ignores the driver's own ancestors in its cargo/rustc check, but any other running build
still refuses it, so build first and start the finished binary.

| Command | Does |
|---|---|
| `corpus [--update-lock]` | builds `vt-bench`, writes the 23 fixtures to `.tmp/compare/corpus`, checks `corpus.lock` |
| `build` | builds the eight Rust adapters and runs `npm ci` for xterm; prints each engine's pin |
| `verify [--engine E] [--fixture F]` | one verify process per pair, prints the status of every pair |
| `golden [--fixture F] [--replace]` | goldens by plurality (3+ engines agree), `golden/ungated.toml` for the rest |
| `run --machine NAME` | verify, time, judge; writes `perf/compare/NAME/<date>.{json,md,svg}` |
| `report --in FILE` | regenerates the `.md` and `.svg` of a results JSON |

`run` sets the High performance power plan and restores the previous one on every exit path
(also after Ctrl-C; a killed driver is repaired at the next start), pins itself and the adapters
to logical CPU 4 (`--cpu N`, `--no-pin`), and refuses while `cargo` or `rustc` runs or the corpus
drifted from `corpus.lock`. A full run takes about 50-80 minutes.

## Where things are

| Path | What |
|---|---|
| `harness/` | adapter side: `Engine` trait, CLI, timing, canonical dump, memory |
| `driver/` | the `compare` binary |
| `engines/<name>/` | one package per engine with its own workspace and lock; `xterm` is Node |
| `corpus.lock` | hashes of the 23 fixtures (corpus version 1) |
| `golden/` | expected states by plurality; `ungated.toml` lists fixtures without a consensus |
| `accepted.toml` | divergences the owner accepted (agents never add entries) |
| `perf/compare/<machine>/` | committed runs; the `.md` carries the methodology |

## Checks (CI does not run them)

```bash
cd compare && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
# in each compare/engines/<rust engine>/: cargo fmt --check && cargo clippy --all-targets -- -D warnings
# in compare/engines/avt: cargo test
cd compare/engines/xterm && node adapter.mjs selftest
```
