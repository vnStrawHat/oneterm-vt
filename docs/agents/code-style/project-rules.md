# Project-specific rules

- **English-only** in code, comments, docs, commit messages, and user-facing strings.
- **Errors:** libraries use `thiserror` domain errors; binaries may use `anyhow` at the top level.
- **Lints** are set once in the root `Cargo.toml` (`[workspace.lints]`); every crate opts in with `[lints] workspace = true`. `unwrap_used`, `todo`, `dbg_macro` are denied outside tests.
- **Quality gate** before reporting work done:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
- **Shell/tooling rules for agents:**
  - Use Git Bash + GNU coreutils (no PowerShell).
  - Scoped searches only: every find/grep/ls targets an explicit folder inside the project. Never scan a drive.
  - Stay inside the project folder: temp files go to `.tmp/`, caches to `.cargo-home/`, build output to the shared `target/` at the project root. The session hook sets `CARGO_HOME`, `CARGO_TARGET_DIR`, `TMP`, `TEMP`, `TMPDIR`; never override them with paths outside the project.
  - Git worktrees live only under `.claude/worktrees/` (the default of the worktree tool). They share the root `target/`, so dependencies build once.
  - Do not commit unless asked.
