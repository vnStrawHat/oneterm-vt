# 0001: Scaffold (phase 0)

## Goal

Turn the template into the `oneterm-vt` workspace: an empty but buildable library crate with its
final feature names, a `vt-bench` tool whose `chart` works, CI on three OSes, a tag-driven release
flow that stores a perf record per version, and the docs skeleton.

## Non-goals

- Any engine code (parser onward is phase 1+; see [roadmap](../../roadmap.md)).
- Measuring anything: `vt-bench` measuring commands are stubs that error.
- Publishing to crates.io (`publish = false` stays), release binaries, SHA-pinned actions.

## Files in this spec

| File | Covers |
|---|---|
| [workspace.md](workspace.md) | removals, root `Cargo.toml` deps, both crate manifests, `lib.rs`, LICENSE |
| [bench-cli.md](bench-cli.md) | `vt-bench` usage, stub behaviour, `chart` SVG design, unit tests |
| [workflows.md](workflows.md) | `.github/workflows/ci.yml` and `release.yml` |
| [release-script.md](release-script.md) | `scripts/release.sh` steps and checks |
| [docs-text.md](docs-text.md) | text for `CHANGELOG.md`, `README.md`, `CLAUDE.md`, `NOTICE` |

Already written by the architect (coder only checks links): `docs/roadmap.md`,
`docs/user/README.md`, `docs/perf/README.md` (perf record schema), `docs/README.md`.

## Files to touch

| Action | Path |
|---|---|
| delete | `crates/app/`, `tools/` |
| edit | `Cargo.toml`, `CLAUDE.md` (header + crate layout only), `README.md` |
| create | `crates/vt/{Cargo.toml,src/lib.rs}` |
| create | `crates/vt-bench/{Cargo.toml,src/main.rs,src/perf_history.rs,src/chart.rs}` |
| create | `.github/workflows/{ci.yml,release.yml}`, `scripts/release.sh` (executable) |
| create | `LICENSE`, `NOTICE`, `CHANGELOG.md` |

## Acceptance criteria

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test --workspace` passes, including every test listed in bench-cli.md
- [ ] `cargo build -p oneterm-vt --no-default-features` passes
- [ ] `RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps --all-features` passes
- [ ] `cargo run -p oneterm-vt-bench -- --help` prints the usage text, exit 0
- [ ] `vt-bench chart` on two hand-made records in `.tmp/history/` writes an SVG that opens in a browser
- [ ] `scripts/release.sh` checks in release-script.md behave as listed; `shellcheck` clean
- [ ] `Cargo.lock` no longer lists `app`, `tracing`, `thiserror`
- [ ] CI green on all three OSes once pushed (owner verifies)

## Open items (owner)

1. `<OWNER>` for NOTICE copyright and `<REPO_URL>` for README / `workspace.package.repository`.
2. Pin actions by SHA instead of major tag? (supply-chain hardening; costs bump churn)
3. Release from `main` only (step 3 of the script) — confirm.
