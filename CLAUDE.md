# oneterm-vt

Embeddable terminal core in Rust (VT parser, grid, reflow, selection, snapshots, input encoders, Sixel, PTY), built phase by phase per `docs/roadmap.md`.

## Crate layout

- `crates/vt` — package `oneterm-vt`, lib: the terminal engine. Features: `pty` (default), `vt-paranoid`, `regex`, `hotpath-profiling`.
- `crates/vt-bench` — package `oneterm-vt-bench`, bin `vt-bench`: benchmark tiers and `chart`. Developer tool, never a dependency of `oneterm-vt`.

Cargo virtual workspace at the root; shared versions, lints, and profiles live in the root `Cargo.toml`.
Docs: `docs/roadmap.md` (plan), `docs/specs/` (per phase), `docs/user/` (embedder guide), `docs/perf/` (perf history). Release: the Release workflow (Actions tab), see docs/user/versioning.md.

## Environment rules

- Work only inside this project folder. Temp files go to `.tmp/`, caches to `.cargo-home/`, build output to `target/` at the root. The SessionStart hook (`.claude/hooks/cargo-env.sh`) exports `CARGO_HOME`, `CARGO_TARGET_DIR`, `TMP`, `TEMP`, `TMPDIR`, `CARGO_BUILD_JOBS` for every Bash call; never override them with paths outside the project. Fallback if a shell missed the hook: `eval "$(bash .claude/hooks/cargo-env.sh)"`.
- Git worktrees only under `.claude/worktrees/` (the default of `EnterWorktree` / `claude --worktree`). They share the root `target/`, so dependencies build once. Check free disk space before creating one.
- Use Git Bash + GNU coreutils for shell work, never PowerShell (the PowerShell tool does not receive the hook's env on Windows).
- Every find/grep/ls must target an explicit folder inside the project; never scan a drive.
- Do not commit unless asked.
- Commits follow `docs/agents/commits.md` (Conventional Commits, one commit per green phase).
- Keep docs short: split by topic into files ≤ ~120 lines with a README.md index.

## Build and disk

- `profile.dev`: own crates `debug = "line-tables-only"`; dependencies `debug = false, opt-level = 1`. Release: thin LTO, stripped.
- One `target/` for every worktree (via `CARGO_TARGET_DIR`); cargo serializes builds across worktrees with a lock. Never start a second cargo build while one is running; wait for it.
- OOM guard: the hook sets `CARGO_BUILD_JOBS = min(cpus, RAM_GB / 3)`. Do not pass `--jobs` or set `CARGO_BUILD_JOBS` higher. If a build still runs out of memory, retry once with `CARGO_BUILD_JOBS=4`.
- Reclaim space: `cargo clean -p <crate>` for one crate, `cargo clean` for everything, `rm -rf .cargo-home/registry/src` for unpacked sources (re-created on demand).

## Quality gate

Before reporting work done:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

## Agents

The main session only orchestrates; see `docs/agents/workflow.md`. Agents live in `.claude/agents/`:

- `architect` (opus) — designs, writes specs to `docs/specs/NNNN-title/` folders; docs-only edits.
- `coder` (sonnet, high) — implements specs with tests and the quality gate.
- `coder-lite` (sonnet, low) — mechanical tasks, gate runs.
- `advisor` (fable) — read-only second-opinion review of specs and diffs.

## Code style

@docs/agents/code-style/README.md
@docs/agents/code-style/project-rules.md
@docs/agents/code-style/format-and-naming.md
@docs/agents/code-style/structure.md
@docs/agents/code-style/design.md
@docs/agents/code-style/errors-async-docs.md
@docs/agents/code-style/testing.md
