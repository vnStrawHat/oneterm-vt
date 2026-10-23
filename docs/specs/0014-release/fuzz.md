# Fuzzing: `fuzz/`

A cargo-fuzz crate at the repository root, **not** a workspace member. It proves the roadmap
rule "hostile input never panics and never grows memory without a bound" beyond what proptest
and the 1M-random-byte test reach. Run by hand before each release; no CI job (Decision 6).

## Why not a member, why Linux

- cargo-fuzz needs nightly and `libfuzzer-sys` (C++ build); the workspace is pinned to stable
  1.96.0 and must build without a C++ compiler. An empty `[workspace]` table keeps it apart,
  so `cargo build --workspace`, `cargo fmt --all` and clippy never see it.
- libFuzzer needs LLVM sanitizer support: Linux and macOS on x86-64 or AArch64, not Windows.
  On the Windows baseline machine, run it in WSL against the same checkout.

## `fuzz/Cargo.toml`

```toml
[package]
name = "oneterm-vt-fuzz"
version = "0.0.0"
publish = false
edition = "2024"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"
oneterm-vt = { path = "../crates/vt", default-features = false, features = ["vt-paranoid"] }

[[bin]]
name = "parser"            # likewise "feed", "resize"
path = "fuzz_targets/parser.rs"
test = false
doc = false
bench = false

[workspace]
```

- `default-features = false`: no `pty`, nothing spawns. `vt-paranoid`: the whole-history
  integrity walk after every mutation turns silent grid corruption into a crash.
- `fuzz/Cargo.lock` is committed (own workspace, reproducible runs).
- Root `.gitignore` gains `/fuzz/corpus/`, `/fuzz/artifacts/`, `/fuzz/coverage/`.
- Each target: `#![no_main]`, module doc (what it fuzzes, the run command), no `unwrap`.

## Targets (`fuzz/fuzz_targets/`)

| Target | Drives | Input use |
|---|---|---|
| `parser.rs` | `parser::Parser::new()` + a `Sink` implementing **every** `Dispatch` method, each passing its arguments (and every param, sub-param and OSC param) to `std::hint::black_box` | split at `data[0] as usize % len` into two `advance` calls, so state survives a chunk boundary |
| `feed.rs` | `Terminal::new(Size { rows: 8, cols: 40 }, config)`, `scrollback_limit: 200`, `allow_screen_readback` from bit 0 of `data[0]`, routes `Forward` for 7501 and `BuiltinAndForward` for 52 | first 8 bytes = chunk lengths (`1 + b % 64`, cycling) over the rest; per chunk `feed` at `start + 50 ms x chunk index` (so the `?2026` timeout path runs), then resolve every span of every event with `batch.str` / `bytes` / `params` into `black_box`; after the last chunk one `snapshot_update` on one `SnapshotState`, then `take_graphics()` |
| `resize.rs` | `Terminal::new(Size { rows: 24, cols: 80 }, ..)`, `scrollback_limit: 200` | op stream: tag `t`; `t & 3 == 0` -> `resize(Size { rows: b1 % 70, cols: b2 }, policy)` with the policy from bit 2 and the next two bytes (0 rows/cols exercise the clamp); else `feed` the next `t >> 2` bytes; one `snapshot_update` after every resize |

No target asserts: a panic, an abort, an OOM or a timeout is the finding.

## The run (per target, before each release, at the commit to be released)

```bash
# once, in Linux or WSL
rustup toolchain install nightly-2026-05-25 --profile minimal
cargo install cargo-fuzz --locked
# seed the byte targets from the bench fixtures (1 MiB each, truncated by -max_len)
cargo run -p oneterm-vt-bench --release -- fixtures --out fuzz/corpus/feed --mib 1
mkdir -p fuzz/corpus/parser && cp fuzz/corpus/feed/* fuzz/corpus/parser/
# each of parser, feed, resize
cargo +nightly-2026-05-25 fuzz run --target-dir target/fuzz-linux feed -- \
    -max_total_time=600 -rss_limit_mb=512 -max_len=4096 -timeout=10
```

- Same nightly as the API snapshot: one pin to bump. `--target-dir target/fuzz-linux` keeps Linux artifacts
  apart from the Windows build in the shared `target/`.
- `-rss_limit_mb=512`: the engine's caps (OSC 8 MiB, DCS cap, intern tables) sum far below it;
  hitting it means a missing bound. `-timeout=10`: a slow input is a quadratic path, a finding.
- `-max_len=4096` keeps `vt-paranoid` walks cheap enough for useful exec rates.

**Clean** = exit 0 after 600 s and no new file in `fuzz/artifacts/<target>/`.

## A finding

1. `cargo +nightly-2026-05-25 fuzz tmin --target-dir target/fuzz-linux <target> <artifact>` to minimise.
2. Turn the minimised bytes into a regression test in `crates/vt` (the module that failed), fix,
   commit as `fix(<scope>): ...` with the test. Never commit the artifact itself.
3. Re-run that target 10 minutes; record both runs in `acceptance.md`.
