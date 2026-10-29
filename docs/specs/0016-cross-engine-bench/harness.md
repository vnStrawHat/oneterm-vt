# Harness: layout, adapter contract, driver, process model

## Layout (`compare/`, outside the root workspace and CI)

```text
compare/
  README.md            fetch, build, run in 10 lines; links here
  fetch-engines.sh     pinned sources, Zig, libghostty-vt build (engines.md)
  corpus.lock          corpus v1 hashes (corpus.md)
  accepted.toml        owner-accepted divergences (verification.md)
  golden/              <fixture>.json, ungated.toml
  Cargo.toml           [workspace] members = ["harness", "driver"], exclude = ["engines"]; own Cargo.lock
  harness/             lib `compare-harness`: adapter side (trait, CLI, timing, dump, memory)
  driver/              bin `compare`: corpus, build, verify, golden, run, report
  engines/<name>/      one package per engine, its OWN [workspace] and Cargo.lock,
                       bin `engine-<name>`; path dep on ../../harness
  engines/xterm/       package.json, package-lock.json, adapter.mjs (Node)
```

- Root `Cargo.toml` gets `exclude = ["compare"]`; root fmt/clippy/test and CI never see it.
  `compare/Cargo.toml` excludes `engines` so cargo never treats an adapter as a member.
- **One engine per package, one workspace per engine**: no feature or version unification
  across engines (each resolves its own `unicode-width`, `vte`, ...), and the probe hit a
  cargo failure when one package path-depended on both `crates/vt` and the wezterm checkout
  (`wezterm-term` then inherited from the wrong workspace root). One engine per package works.
- All share `CARGO_TARGET_DIR` (`target/`); binary names are unique (`engine-<name>.exe`).
- Every Rust package copies the root `[profile.release]` (criteria.md "build").
- Lints: `compare/Cargo.toml` `[workspace.lints]` copies the root table; each engine package
  copies it too; `ghostty` and `libvterm` lift `unsafe_code` with a one-line reason (FFI).

## Adapter contract (harness lib)

```rust
pub struct Geometry { pub cols: u16, pub rows: u16, pub scrollback: usize }
pub enum Capability { Sixel, Graphemes, Hyperlinks }
pub struct EngineInfo {
    pub name: &'static str, pub version: &'static str, pub source: &'static str,
    pub language: &'static str, pub build: &'static str, pub capabilities: &'static [Capability],
}
pub trait Engine: Sized {
    const INFO: EngineInfo;
    fn new(geometry: Geometry) -> Self;
    fn feed(&mut self, bytes: &[u8]);
    /// Flush deferred work (pending `?2026` buffer, async queue). Timed after the last
    /// feed; in `verify` also called before EVERY checkpoint dump.
    fn finish(&mut self) {}
    fn dump(&self, out: &mut DumpBuilder);
}
pub fn main<E: Engine>() -> ExitCode;
```

`DumpBuilder` (harness): `cursor(row, col)`, `row(|cells| ...)` taking
`Cell { text, wide: bool, spacer: bool, fg, bg, flags, uri }` per column (`text` = base char
plus any combining marks the engine stores beside it), `history_row(...)`;
it applies every rule of verification.md "Canonical dump". A trait is justified here: nine
implementations.

## Adapter CLI (same for the Node adapter)

```text
engine-<name> info
engine-<name> grid    --file F [--passes 5] [--warmup 1]
engine-<name> verify  --file F [--checkpoints 4]
common: [--cols 160] [--rows 45] [--scrollback 10000] [--chunk 65536]
```

stdout: exactly one JSON object; stderr free; exit 0, or 2 on bad args, 1 on engine error.
`grid` -> `{bytes, samples_ns: [..], final_sha256}`; `verify` -> `{dumps: [4], final_sha256,
memory: {base, steady, peak}}`; `info` -> `EngineInfo` as JSON.

`verify` order per checkpoint: feed chunks up to the checkpoint, `finish()`, then dump. After
the last checkpoint's `finish()`: sample memory (`steady`, `peak`), then dump.

## Driver (`compare`)

| Command | Does |
|---|---|
| `corpus [--mib 32] [--update-lock]` | builds `oneterm-vt-bench` (release) in the root workspace, runs `vt-bench fixtures --compare --out .tmp/compare/corpus`, checks `corpus.lock` |
| `build` | `cargo build --release` in each `engines/<rust>` dir, one after another (never parallel); `npm ci` in `engines/xterm` with `npm_config_cache=.cargo-home/npm` |
| `verify [--engine E] [--fixture F]` | verify processes -> `.tmp/compare/verify/<engine>/<fixture>.json`, prints statuses |
| `golden [--fixture F] [--replace]` | verification.md "Goldens" (no golden without plurality) |
| `run --machine NAME [--engine E] [--fixture F] [--repetitions 3] [--keep-power-plan]` | refuses on corpus drift or a running cargo/rustc; switches the power plan (criteria.md), pins itself; verify, timed grid runs; restores the plan; writes output.md files |
| `report --in FILE` | regenerates `.md` and `.svg` from a results JSON |

Engines run in this fixed order (rotated per repetition): `oneterm`, `ghostty`, `alacritty`,
`rio`, `wezterm`, `vt100`, `avt`, `libvterm`, `xterm`. The driver knows each engine's command
(`target/release/engine-<name>.exe`, or `node compare/engines/xterm/adapter.mjs`).

## Process model and pinning

- The driver calls `SetProcessAffinityMask(current, 0x10)` once at start (children inherit
  affinity) and spawns each adapter with `HIGH_PRIORITY_CLASS`
  via `CommandExt::creation_flags`. Both behind `#[cfg(windows)]` at the FFI call;
  `--cpu N` overrides the logical CPU; `--no-pin` labels the run `unpinned`.
- Timeouts: 600 s per process (`wait_timeout` loop on `try_wait`); a killed process is `failed`.
- Memory in the adapter: `K32GetProcessMemoryInfo` (`PrivateUsage`, `PeakPagefileUsage`) on
  Windows, `/proc/self/status` (`VmRSS`, `VmHWM`) elsewhere; Node: `process.memoryUsage().rss`
  after `global.gc()` (`--expose-gc`), labelled `rss`.
- Driver dependencies (compare workspace only): `serde`, `serde_json`, `sha2`, `toml`,
  `anyhow`, `windows-sys`. Harness lib: `serde`, `serde_json`, `windows-sys`, `sha2`.
