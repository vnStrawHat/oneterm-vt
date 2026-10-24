# 0014 acceptance (coder part, 2026-10-24, Windows 11, stable 1.96.0)

Everything below the line "Pending" is not done and is not claimed.

## Pins and tools used

| Item | Value |
|---|---|
| nightly | `nightly-2026-05-25` (rustc 1.98.0-nightly, 2026-05-24); built rustdoc JSON for `oneterm-vt` without walking back |
| `cargo-public-api` | `0.52.0`, installed with `--locked` into `.cargo-home/bin` |
| standard libraries on the nightly | `x86_64-pc-windows-msvc` (host), `x86_64-unknown-linux-gnu` |
| `shellcheck` | **not installed here; it did not run.** `scripts/public-api.sh` and `scripts/release.sh` were read for findings and pass `bash -n`; CI step "Shell scripts" is the real check |

## M1 `release.sh` kinds

Run in a throwaway clone (`git clone . .tmp/rel-clone`, edited script copied in, tree dirty, `VT_BENCH_MACHINE=x`); the main tree was not touched, no tag was created.

| Workspace | Run | Observed |
|---|---|---|
| 0.1.0 | `patch` | `releasing v0.1.0 -> v0.1.1`, exit 1 (dirty tree) |
| 0.1.0 | `minor` | `releasing v0.1.0 -> v0.1.1`, exit 1 |
| 0.1.0 | `major` | `releasing v0.1.0 -> v0.2.0`, exit 1 |
| 0.1.0 | `0.1.0` | `releasing v0.1.0 -> v0.1.0`, exit 1 |
| 0.1.0 | `0a1b2` | usage, exit 2 |
| 0.1.0 | `1.2.3x` | usage, exit 2 (dots are escaped now) |
| 1.2.3 (clone only) | `patch` / `minor` / `major` | `v1.2.4` / `v1.3.0` / `v2.0.0` |

`.github/workflows/release.yml` and `cliff.toml`: unchanged (`git diff` empty).

## M2 snapshot vs roadmap lists

- Both files generated: `crates/vt/public-api.unix.txt` (3285 lines), `public-api.windows.txt` (3295 lines).
  They differ exactly as `public-api.md` says: `pty::SignalMask`, `Options::child_signal_mask` (Unix only);
  `pty::PipeReader`, `PipeWriter`, `Options::escape_args` (Windows only); the associated `PseudoConsole::Reader` / `Writer`
  types and `reader()` return type follow (`std::fs::File` on Unix).
- Every item named in a roadmap "Public:" line is present (root items, `parser::*` list, `search::*` list, the
  `Terminal` methods `snapshot_update`, `sync`, `mode_snapshot`, `encode_key`, `encode_key_event`, `take_graphics`,
  `placements`; the Windows-only `pty` items are in the Windows file only).
- Root-level types and functions in the snapshot but not named one by one in a roadmap list: **none**.
  `SnapshotContent` and `SnapshotCursor` are covered by the `Snapshot*` wildcard of phase 7.
- Outside the lists but allowed by module wildcards: everything under `grid::*`, `intern::*`, `input::*`, `pty::*`
  (including `pty::EventedPty`, `EventedReadWrite`, `OnResize`, `ChildEvent`, `GlyphWidth`, `PTY_*_TOKEN`).
- `VtEvent` is `#[non_exhaustive]`; `Config` is not (a new field is a documented minor change; see "Deviations").

## M3 drift is caught

- A dummy `pub fn dummy_drift()` in `lib.rs`: `scripts/public-api.sh check` printed the diff and the `::error::` line, exit 1.
  After the revert: exit 0.
- `scripts/public-api.sh update` run twice: both files byte-identical to the first generation (`cmp`).
  Cross generation (`--target x86_64-unknown-linux-gnu` from Windows) is not yet compared with a native Linux run.

## M4 fuzz: **pending, must be done on Linux (or WSL)**

- Not run here: cargo-fuzz is unavailable on Windows (Decision 6) and `libfuzzer-sys` cannot be compiled here
  (its build script needs a Linux C++ compiler: `failed to find tool "x86_64-linux-gnu-g++"`).
- What was verified instead, with a stand-in `libfuzzer-sys` whose `fuzz_target!` defines a plain function
  (the real target files, unmodified, in a scratch crate under `.tmp/`):
  - `cargo +nightly-2026-05-25 check --target x86_64-unknown-linux-gnu`: the three targets type-check.
  - `cargo clippy -- -D warnings` (stable 1.96.0): clean. `cargo fmt --manifest-path fuzz/Cargo.toml -- --check`: clean.
  - Smoke run on the Windows host (not coverage guided, features as in `fuzz/Cargo.toml`, `vt-paranoid` on):
    60 000 pseudo-random inputs (escape-heavy and plain random, up to 4 KiB) through all three targets, about 196 s:
    no panic, no timeout.
- Not verified: that the real `libfuzzer-sys` macro accepts `|data: &[u8]|` in this form (it does in 0.4), the
  `cargo fuzz run` commands, and everything about throughput, memory and coverage.

| Target | Seconds | Execs | Peak RSS | Corpus | Artifacts |
|---|---|---|---|---|---|
| parser | pending | pending | pending | pending | pending |
| feed | pending | pending | pending | pending | pending |
| resize | pending | pending | pending | pending | pending |

## M5 docs

`wc -l docs/user/*.md`: every file is at most 120 lines except `api-map.md` (149, limit 170); `for-agents.md` is 84 (limit 100).
`crates/vt/tests/user_guide.rs` passes (wiring of all 20 `docs/user/*.md`; every public type of both snapshots has a row in `api-map.md`);
the doctest run includes every chapter and the root `README.md` (`readme` module). Removing a row from `api-map.md` makes
the test fail with the missing names (checked).

## What ran (gate, this machine)

Gate commands that ran: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; clippy with `--no-default-features`; `cargo test --workspace`; test with `--no-default-features`; test with `--all-features`; `cargo doc` with and without `--all-features`; headless example with and without default features; `scripts/public-api.sh check`; `git diff --check`.

## Deviations from the spec text

1. `examples/headless.rs`: `main` returns `std::io::Result<()>` and writes through a locked stdout with `writeln!` and `?`,
   instead of `#![allow(clippy::print_stdout)]` plus `println!` (the task forbids `#[allow]`). No `unwrap` / `expect`.
2. `api-map.md` folds small types around one idea into one row (a type with its id, an enum with its payload) to stay within
   170 lines; every public type is still named in backticks, and the test enforces it.
3. `tests/user_guide.rs` also trims a trailing `:` from the item path (`pub trait path: Super`, e.g. `pty::EventedPty`).
4. CI runs `bash scripts/public-api.sh check` (the spec's bare path needs the executable bit in the index; the owner sets it
   with `git add scripts/public-api.sh && git update-index --chmod=+x scripts/public-api.sh` at commit time, which a
   Windows checkout cannot do).
5. `scripts/public-api.sh` installs the nightly only when it or a target is missing (so a local run with a custom `CARGO_HOME`
   does not trip the rustup self-check), and puts `${CARGO_HOME:-$HOME/.cargo}/bin` on `PATH`.
6. Spec inconsistency, for the architect: the snapshot table says a new `pub` field on a struct without `#[non_exhaustive]` is
   major, but `Config` has no `#[non_exhaustive]` and its documentation (and the new `versioning.md` bullet) says a new field is
   minor. `versioning.md` names `Config` as the exception; decide whether `Config` should become `#[non_exhaustive]` before 1.0.

## Pending (owner)

- Fuzz: three targets x 10 minutes on Linux/WSL at the release commit (fuzz.md), then fill the M4 table.
- First native Linux CI run of `scripts/public-api.sh check` (and Windows): if the cross-generated Unix file differs from
  the native one, apply the diff the CI step prints.
- Repository URL: replace `<your fork url>` in `README.md` ("Use") and `docs/user/versioning.md` (pin example); `git remote add origin`.
- `VT_BENCH_MACHINE` value (release.md step 7 suggests `i7-12700 12C/20T, Windows 11 26200, rustc 1.96.0`; confirm).
- `git update-index --chmod=+x scripts/public-api.sh` when committing.
- Run `scripts/release.sh 0.1.0` on the quiet baseline machine, `grid --check`, push, and record steps 3-13 of release.md
  (tag sha, release URL, run ids) in this file.

## Release checklist (release.md steps 3-13)

| Step | Date | Result |
|---|---|---|
| 3 push, CI green on 3 OS | pending | |
| 4 API review | 2026-10-24 | M2 above |
| 5 fuzz | pending | |
| 6 perf verdict | pending | |
| 7 `scripts/release.sh 0.1.0` | pending | |
| 8 `grid --check`, chart | pending | |
| 9-13 push, Release run, notes, assets, record | pending | |
