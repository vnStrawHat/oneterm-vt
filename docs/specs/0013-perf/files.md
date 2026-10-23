# Files, order of work, commits

Builds on specs 0001-0012 as implemented (tier 3 `render.rs` from 0008; all ten fixtures in
tiers 2-3 from 0011). Sub-milestones, gate green after each:
**12a** hotpath feature, sites, bench features and guard, CI line; **12b** `--fixture`,
`--check`, the `--json` guard, tests; **12c** target runs, profiles, fixes or waivers;
**12d** docs, release dry run, `acceptance.md`.

## Code

| Path | Change |
|---|---|
| `crates/vt/Cargo.toml` | `hotpath-profiling = ["dep:hotpath"]` with the hotpath.md comment; `hotpath = { workspace = true, optional = true }` |
| `crates/vt/src/terminal.rs` | `cfg_attr` sites on `feed`, `snapshot_update` |
| `crates/vt/src/parser/mod.rs` | `cfg_attr` site on `Parser::advance` |
| `crates/vt/src/grid/screen.rs` | `cfg_attr` sites on `Screen::scroll_up`, `Screen::scroll_down` |
| `crates/vt-bench/Cargo.toml` | `[features]` `vt-paranoid`, `hotpath-profiling` (hotpath.md); optional `hotpath` dependency |
| `crates/vt-bench/src/regression_check.rs` | new: check.md "Code" table + its tests |
| `crates/vt-bench/src/main.rs` | `mod regression_check;`; `hotpath::main` `cfg_attr`; USAGE; `grid --check` routing and exit mapping; `--fixture` flag; `--json` guard |
| `crates/vt-bench/src/fixtures.rs` | `select(name)` |
| `crates/vt-bench/src/throughput.rs`, `render.rs` | take the selected fixtures |
| `.github/workflows/ci.yml` | one line in "Without default features" (hotpath.md CI) |
| root `Cargo.toml` | nothing: `hotpath = "0.26"` and `[profile.release]` stay as they are (**D4**) |
| `scripts/release.sh` | nothing (history.md) |

USAGE after this phase (the `Measure` block; the rest unchanged):

```text
Measure:
  all | parser | grid | render | resize | rss
      [--mib N] [--frames N] [--json] [--out FILE] [--machine TEXT]
      [--fixture NAME]                 parser, grid, render only: one fixture, no --json
  fixtures --out DIR [--mib N]     write the ten benchmark streams as DIR/<name>.vt
  grid --check [--baseline FILE] [--tolerance R] [--mib N]
                                   compare tier 2 with the newest perf/history record
```

"(lands phase by phase; see the roadmap)" is dropped from the `Measure:` header: every
command exists now.

## Docs

| Path | Change |
|---|---|
| `docs/user/performance.md` | new chapter 12 (history.md outline) |
| `docs/user/README.md` | row 12 -> link + `ready` (the Features row for `hotpath-profiling` already fits) |
| `docs/perf/README.md` | Commands, Waivers, tiers 3-5 line, Rules bullet (history.md table) |
| `docs/specs/0013-perf/acceptance.md` | new, by the coder (targets.md "Acceptance notes") |

## Rules

- No new public item in `oneterm-vt`; `hotpath` is never re-exported and never named in rustdoc
  outside the feature comment.
- No `unwrap`/`expect`/`panic!` outside tests; `--check` errors are `anyhow` errors with the
  check.md texts; printing stays in `main.rs::run` (the one `print_stdout` allow).
- No `#[allow]` for code inside the hotpath expansion; report instead (hotpath.md).
- Floats: compare with `<` exactly as check.md states; no epsilon.

## Gate

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy -p oneterm-vt --all-targets --features hotpath-profiling -- -D warnings
cargo test --workspace
cargo test -p oneterm-vt --no-default-features
cargo test -p oneterm-vt --features hotpath-profiling
RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps --all-features
```

## Commits (order is fixed)

1. Tooling commit (12a, 12b, 12d, plus the 12c step-1 table and profile summaries in
   `acceptance.md`): `feat(bench): grid --check trip-wire and hotpath-profiling sites` (minor:
   the engine's `hotpath-profiling` feature starts measuring). Body: `Spec: docs/specs/0013-perf`.
2. One commit per fix: `perf(<scope>): <what>, <fixture> <before> -> <after> MiB/s` (patch).
   It also updates `acceptance.md` (re-run table) and, if the figure moved, the table in
   `docs/user/performance.md`.
3. Waiver rows go into the commit that closes the phase: the last `perf` commit, or, when the
   owner waives without a further fix, `docs(perf): record tier-2 waivers for <fixtures>`.

Phase 12 is done when every fixture passes or is waived; the orchestrator reports which.
