# Root text files

Copy each block verbatim (drop the fence). `<OWNER>` and `<REPO_URL>` are open items: keep the
placeholder if the owner has not answered when the coder starts.

## `CHANGELOG.md`

````markdown
# Changelog

All notable changes to `oneterm-vt`, in [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
form. A change gets an entry when it alters what an embedder compiles against or the bytes the
terminal sends back.

## Versioning promise

Releases are git tags `vX.Y.Z`; consumers depend by tag. While the crate is `0.x`, a minor bump
is breaking, exactly as Cargo reads it.

1. **Minor bump** with an entry naming the item: removing an item, changing a signature, adding a
   variant to an exhaustive enum, adding a field to a struct built by struct literal.
2. **Patch bump**: a new method, module or default-off feature, or a new variant on a
   `#[non_exhaustive]` enum.
3. Raising the minimum supported Rust version is a minor bump, never a patch.
4. `parser::Dispatch` is an engine-internal contract exposed for advanced use; its method set may
   change in a patch.
5. Items behind a non-default feature carry the same promise as the default surface.
6. **Reply bytes are API.** The replies to DA1, DA2, DA3, DSR, DECRQM, XTVERSION, DECRQCRA,
   DECRQSS, XTGETTCAP and the OSC colour queries, and the bytes the `input` encoders produce for a
   given (event, mode snapshot) pair, change only in a minor bump with an entry. Programs parse
   them. `Config::product_name` changes the identity part of a reply, never its shape.
7. The meaning of each `FeedStats` counter is API; counting something different is a minor bump.

Not promised: grid internals reachable through `grid::Screen`, exact `SeqNo` values, allocation
behaviour, performance. Promised: a `RowId` names the same content for as long as that content is
live, because embedder caches key on it.

## [Unreleased]

### Added

- Workspace scaffold: the `oneterm-vt` crate with features `pty` (default), `vt-paranoid`,
  `regex` and `hotpath-profiling`, all empty for now; no public items yet.
- `vt-bench chart`: an SVG of tier-2 throughput across releases, from `perf/history/*.json`.
- CI on Linux, Windows and macOS; tag-driven GitHub releases; `scripts/release.sh`.
````

## `README.md` (replaces the template text)

````markdown
# oneterm-vt

An embeddable terminal core in Rust: VT parser, grid with scrollback, reflow, selection,
damage-tracked snapshots, key and mouse encoders, Sixel, and an optional pseudo-console.
You bring the pixels.

**Status:** under construction; see [docs/roadmap.md](docs/roadmap.md).

## Use

```toml
[dependencies]
oneterm-vt = { git = "<REPO_URL>", tag = "v0.1.0" }
```

Guide: [docs/user/](docs/user/README.md). Semver promise: [CHANGELOG.md](CHANGELOG.md).

## Layout

| Path | What |
|---|---|
| `crates/vt` | the library, package `oneterm-vt` |
| `crates/vt-bench` | `vt-bench`: benchmark tiers and the perf-history chart (not shipped) |
| `docs/user/` | embedder guide |
| `docs/perf/`, `perf/` | how performance is recorded; per-release records and chart |
| `docs/roadmap.md`, `docs/specs/` | build plan and per-phase specs |
| `scripts/release.sh` | bump, benchmark, chart, changelog, commit, tag |

## Develop

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

Working with Claude Code: see `CLAUDE.md` and `docs/agents/`.

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
````

## `CLAUDE.md` edits

Replace the first two lines and the `## Crate layout` section; leave everything else.

```markdown
# oneterm-vt

Embeddable terminal core in Rust (VT parser, grid, reflow, selection, snapshots, input encoders, Sixel, PTY), built phase by phase per `docs/roadmap.md`.

## Crate layout

- `crates/vt` — package `oneterm-vt`, lib: the terminal engine. Features: `pty` (default), `vt-paranoid`, `regex`, `hotpath-profiling`.
- `crates/vt-bench` — package `oneterm-vt-bench`, bin `vt-bench`: benchmark tiers and `chart`. Developer tool, never a dependency of `oneterm-vt`.

Cargo virtual workspace at the root; shared versions, lints, and profiles live in the root `Cargo.toml`.
Docs: `docs/roadmap.md` (plan), `docs/specs/` (per phase), `docs/user/` (embedder guide), `docs/perf/` (perf history). Release: `scripts/release.sh <version>`.
```

## `NOTICE`

```text
oneterm-vt
Copyright 2026 <OWNER>

Licensed under the Apache License, Version 2.0 (the "License"); you may not use
this software except in compliance with the License. A copy is in the LICENSE
file and at http://www.apache.org/licenses/LICENSE-2.0.
```

Third-party notices are added by the phase that introduces third-party code or data.
