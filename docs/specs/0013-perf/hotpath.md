# `hotpath-profiling`: measurement sites and the profile recipe

Sources: the public `hotpath` 0.26 crate docs (`measure`, `main`, `HotpathGuardBuilder`,
`HOTPATH_OUTPUT_PATH`). Workspace pin `hotpath = "0.26"` exists since phase 0.

## Engine feature (`crates/vt/Cargo.toml`)

```toml
# Function timing sites (`hotpath::measure`) on `feed`, the parser, the scroll primitives and
# `snapshot_update`. Off by default; changes no public item. Alone it compiles hotpath's no-op
# half: a site measures only when the final binary also enables `hotpath/hotpath`.
hotpath-profiling = ["dep:hotpath"]

[dependencies]
hotpath = { workspace = true, optional = true }   # feature `hotpath-profiling` only
```

## Sites (the complete committed set)

| Function | File | Attribute |
|---|---|---|
| `Terminal::feed` | `src/terminal.rs` | `#[cfg_attr(feature = "hotpath-profiling", hotpath::measure(impl_type = "Terminal"))]` |
| `Terminal::snapshot_update` | `src/terminal.rs` | same, `impl_type = "Terminal"` |
| `Parser::advance` | `src/parser/mod.rs` | same, `impl_type = "Parser"` |
| `Screen::scroll_up` | `src/grid/screen.rs` | same, `impl_type = "Screen"` |
| `Screen::scroll_down` | `src/grid/screen.rs` | same, `impl_type = "Screen"` |

- `impl_type` puts the type in the reported name (`...::Terminal::feed`); the macro cannot see
  the enclosing `impl` itself (hotpath docs).
- Nesting is real: `feed` contains `advance`, which contains the scrolls. Self time of `feed`
  is `feed - advance`; read the report as a tree, not a sum.
- Sites sit on calls that happen per chunk (`feed`, `advance`), per frame (`snapshot_update`)
  or per scrolled line (the two scrolls). Nothing per byte or per cell: a site costs two clock
  reads, which would swamp a per-cell function.
- An investigation may add temporary sites (or `hotpath::measure_block!`) locally; they are
  removed before commit. A site that proves useful twice joins this table through a spec edit.
- If clippy flags code inside the expansion, report it to the architect; no blanket `allow`.

## The no-op build

| Build | `hotpath` crate half | Sites |
|---|---|---|
| default | not compiled | attribute absent (`cfg_attr` false) |
| `-p oneterm-vt --features hotpath-profiling` | off half (macros return the item unchanged) | compiled, zero cost |
| bench `--features hotpath-profiling` (enables `hotpath/hotpath`) | on half, unified for the whole graph | measuring |

An embedder who turns on `hotpath-profiling` pays nothing until their own binary enables
`hotpath/hotpath` and holds a guard (`#[hotpath::main]` or `HotpathGuardBuilder`); without a
guard the on half records nothing.

## Bench feature and guard (`crates/vt-bench`)

```toml
[features]
# Passthroughs: the bench must know how the engine was built (check.md refusals 3-4).
vt-paranoid = ["oneterm-vt/vt-paranoid"]
hotpath-profiling = ["oneterm-vt/hotpath-profiling", "dep:hotpath", "hotpath/hotpath"]

[dependencies]
hotpath = { workspace = true, optional = true }
```

`main.rs`:

```rust
#[cfg_attr(feature = "hotpath-profiling", hotpath::main(percentiles = [50, 99]))]
fn main() -> ExitCode { /* unchanged */ }
```

The guard lives for the whole of `main`; its report prints on stdout after vt-bench's own
report, or to the file in `HOTPATH_OUTPUT_PATH`. No `--profile` flag (**D5**): the sites exist
only in a binary compiled with the feature, so a runtime flag could not switch them on, and the
feature alone already is the switch. hotpath's allocation mode (`hotpath-alloc`) is not used: it
installs its own global allocator, and vt-bench already has one (tier 5).

## `--fixture NAME` (D6)

hotpath aggregates per function over the whole process, so a profile across ten fixtures
cannot be attributed to one. `parser`, `grid` and `render` gain `--fixture NAME`: run only that
fixture. `measure_tier` and `measure_render` take the selected `&[&Fixture]` instead of walking
`FIXTURES`; `fixtures::select(name: Option<&str>) -> Result<Vec<&'static Fixture>>`.

| Case | Result (exit 1) |
|---|---|
| unknown name | `unknown fixture 'x'; one of: plain_ascii, long_lines, ...` (table order) |
| with `all`, `resize`, `rss`, `fixtures` | `unknown flag '--fixture' for '<cmd>'` |
| with `--json` | `--fixture cannot be used with --json: a record holds every fixture` |

## Recipe (goes into `docs/perf/README.md` "Commands")

```bash
# where does heavy_sgr spend its time?
cargo run -p oneterm-vt-bench --release --features hotpath-profiling -- grid --fixture heavy_sgr --mib 32
# per frame (tier 3 reaches snapshot_update)
cargo run -p oneterm-vt-bench --release --features hotpath-profiling -- render --fixture dense_cells
```

Below the four sites a sampling profiler on the plain release binary gives line detail;
cargo's per-profile environment overrides keep symbols without a new profile:
`CARGO_PROFILE_RELEASE_STRIP=false CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build -p oneterm-vt-bench --release`.

## CI (`.github/workflows/ci.yml`)

- On half: already compiled on all three OSes by `cargo clippy --workspace --all-targets
  --all-features` (bench feature unifies `hotpath/hotpath`), and tested on Linux by
  `cargo test --workspace --all-features` (no guard, so sites record nothing).
- Off half: the Linux step "Without default features" gains
  `cargo clippy -p oneterm-vt --all-targets --features hotpath-profiling -- -D warnings`.
