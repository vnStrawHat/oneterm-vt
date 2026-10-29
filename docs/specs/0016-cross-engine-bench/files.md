# Files, order of work, gate, commits

Sub-milestones, each green before the next:
**16a** vt-bench compare-only fixtures 15-23 + vendored data; **16b** `compare/` workspace, harness,
driver (with the power-plan guard), `oneterm` adapter, corpus/verify/golden on `oneterm`
alone; **16c** the other eight adapters, `fetch-engines.sh`; **16d** first full run, goldens,
`ungated.toml`, the first committed run, `acceptance.md`.

## Root and vt-bench

| Path | Change |
|---|---|
| `Cargo.toml` (root) | `[workspace] exclude = ["compare"]`; nothing else |
| `.gitignore` | `/compare/engines/xterm/node_modules/` |
| `crates/vt-bench/src/fixtures.rs` | `FIXTURES` unchanged (14); new `COMPARE_FIXTURES: [Fixture; 9]` (corpus.md); `select(name)` searches both; `write_fixtures` takes the set; tests V1-V6, V8 |
| `crates/vt-bench/src/fixture_effects_tests.rs` | V7 |
| `crates/vt-bench/data/vtebench/` | `symbols`, `vim_session`, `LICENSE-APACHE`, `LICENSE-MIT`, `README.md` (source, commit, unchanged) |
| `crates/vt-bench/src/main.rs` | `fixtures --compare` flag; USAGE line `fixtures --out DIR [--mib N] [--compare]` with "--compare adds the 9 cross-engine streams"; tiers untouched |
| `docs/perf/README.md` | one row in the path table: `perf/compare/<machine>/` holds cross-engine runs, method in `compare/README.md`; fixture list unchanged (14); no numbers |
| root `README.md`, `docs/user/*` | nothing: cross-engine results are not quoted there |

Release tiers, release bench, `grid --check`, history records and charts are unchanged:
they iterate `FIXTURES` only.

## compare/ (new)

| Path | Content |
|---|---|
| `compare/README.md` | prerequisites (Git Bash, Rust 1.96, Node 22, MSVC Build Tools; Zig is fetched), 5 commands, where results go |
| `compare/fetch-engines.sh` | pins at the top; clones with `--filter=blob:none` + checkout, no submodules; Zig download + sha256 check; libghostty-vt build with the proxy fallback (engines.md); idempotent (skips what exists at the right commit) |
| `compare/Cargo.toml`, `Cargo.lock` | workspace `harness`, `driver`; lints and release profile copied from root |
| `compare/harness/src/{lib.rs, engine.rs, dump.rs, timing.rs, memory.rs, cli.rs}` | harness.md contract; H1-H12 beside the code |
| `compare/harness/tests/dump_cases.json` | X1 cases |
| `compare/driver/src/{main.rs, corpus.rs, build.rs, verify.rs, golden.rs, run.rs, machine.rs, report.rs, chart.rs}` | harness.md driver; D1-D9 |
| `compare/engines/{oneterm, ghostty, alacritty, rio, wezterm, vt100, avt, libvterm}/` | `Cargo.toml` (own `[workspace]`, exact pins, release profile), `Cargo.lock`, `src/main.rs`; `ghostty` and `libvterm` add `build.rs` |
| `compare/engines/xterm/{package.json, package-lock.json, adapter.mjs}` | Node adapter, same CLI, `selftest` for X1 |
| `compare/corpus.lock`, `compare/accepted.toml` (empty table), `compare/golden/*.json`, `compare/golden/ungated.toml` | 16b/16d |
| `perf/compare/<machine>/<date>.{json,md,svg}` | written by `run`; committed (the `.md` carries the methodology) |

## Gate

Root gate unchanged (fmt, clippy `--all-features -D warnings`, `cargo test --workspace`); it
covers 16a. For `compare/`, run in `compare/` and in each `compare/engines/<rust>/`:
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`; `cargo test` in `compare/`;
`node adapter.mjs selftest` in `engines/xterm`. CI does not run any of these (D3).

## Commits (docs/agents/commits.md)

One phase commit, green, after 16d (goldens, lock and the first run included):
`feat(bench): cross-engine comparison harness and nine aligned fixtures` (body:
`Spec: docs/specs/0016-cross-engine-bench`). If the spec is committed earlier:
`docs(spec): ...` first. Later runs: `chore(bench): compare run <machine> <date>` (not
`perf`: nothing in the engine changed).
