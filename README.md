# Claude Rust Template

Starting point for a Rust workspace developed with Claude Code: coding rules, subagents, an agent workflow, and a build setup that keeps everything (caches, temp files, worktrees, build output) inside the project folder.

## Use it

1. Copy the folder (or `git clone`), then `git init` if you copied.
2. Rename `crates/app` and fill in the header of `CLAUDE.md`.
3. Open Claude Code in the folder. The SessionStart hook prints `cargo-env: root=...` once; cargo then uses `.cargo-home/` and `target/` here, temp files go to `.tmp/`.
4. Verify: `cargo test --workspace`, then ask a subagent (e.g. `coder-lite`) to run `echo $CARGO_TARGET_DIR`. It must print `<project>/target`; if it prints nothing, subagents in your Claude Code version do not inherit the hook env, and the fallback line in each agent prompt (`eval "$(bash .claude/hooks/cargo-env.sh)"`) takes over.

## What is inside

| Path | Purpose |
|---|---|
| `CLAUDE.md` | Rules the session and every agent follow |
| `.claude/settings.json` | SessionStart hook + permission allowlist for cargo/git read-only commands |
| `.claude/hooks/cargo-env.sh` | Exports `CARGO_HOME`, `CARGO_TARGET_DIR`, `TMP*` pointing into the project; resolves the main checkout from a worktree; caps `CARGO_BUILD_JOBS` by RAM (OOM guard); warns when the drive has under 20G free |
| `.worktreeinclude` | Untracked files copied into every new worktree |
| `.claude/agents/` | `architect`, `coder`, `coder-lite`, `advisor` |
| `docs/agents/workflow.md` | Who does what, in which order |
| `docs/agents/code-style/` | Rust conventions, split by topic |
| `docs/specs/` | One folder per work item, written by `architect` |
| `Cargo.toml` | Workspace, shared lints, dev/release profiles tuned for build time and disk |
| `rust-toolchain.toml` | Pinned toolchain with rustfmt and clippy |

## Worktrees

`EnterWorktree` / `claude --worktree <name>` creates `.claude/worktrees/<name>/` (git-ignored). The hook resolves the main checkout with `git rev-parse --git-common-dir`, so every worktree builds into the root `target/` and reads the root `.cargo-home/`. Dependencies compile once; only your own crates rebuild per worktree.
