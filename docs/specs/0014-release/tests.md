# Tests and checks

## Automated (run by `cargo test --workspace` and CI)

**T1 + T2: `crates/vt/tests/user_guide.rs`** (integration test; reads files only, uses no crate
API; paths from `env!("CARGO_MANIFEST_DIR")`; plain `std`, no new dev-dependency):

| Test | Behaviour |
|---|---|
| `every_user_doc_is_a_doctest_module` | for every `docs/user/*.md`, `src/lib.rs` contains `include_str!("../../../docs/user/<file>")`; the failure lists the missing files |
| `api_map_names_every_public_type` | for both `public-api.{unix,windows}.txt`: collect module paths from `pub mod <path>` lines; for each line that, after leading `#[...] ` attributes, starts with `pub struct`, `pub enum`, `pub trait`, `pub type` or `pub union`, take the path (cut at the first `<`, `(`, ` `, `;`), keep it only when its parent is a module (skips associated types in impls), strip `oneterm_vt::`; `docs/user/api-map.md` must contain it in backticks (`` `grid::Screen` ``, `` `Terminal` ``); the failure lists every missing name |

Both are one assertion over a collected list, so one run shows every gap.

**T3 (13g): `vt-bench` unit tests** for the platform key, record field, chart per platform and
check baseline per platform: the table in platforms.md "Tests".

**Doctests:** every Rust block of every `docs/user/*.md` and of the root `README.md`, through
the `#[cfg(doctest)]` modules (files.md). `for-agents.md`'s loop asserts the DA1 reply, the row
text and `\x1bOA`.

**CI steps added** (files.md):

| Step | OS | Fails when |
|---|---|---|
| Public API snapshot: `scripts/public-api.sh check` | Linux, Windows | the generated surface differs from the committed file |
| Headless example: `cargo run -p oneterm-vt --example headless` | all three | it does not build, panics or exits non-zero |
| Shell scripts: `shellcheck scripts/*.sh` (existing) | Linux | `public-api.sh` or the edited `release.sh` has a finding |

`cargo clippy --workspace --all-targets` (existing) lints the example; the `--no-default-features`
clippy line (existing) builds it without `pty`.

## Manual checks (coder, results in `acceptance.md`)

**M1 `release.sh` sub-commands.** `bump` prints `releasing vOLD -> vNEW`, then refuses outside
GitHub Actions (`releases are cut by the Release workflow ...`), so a local run prints the
mapping, exits 1 and modifies nothing:

| Run (workspace at `0.1.0`, `VT_BENCH_MACHINE=x`, `GITHUB_ACTIONS` unset) | Prints |
|---|---|
| `scripts/release.sh bump patch` | `releasing v0.1.0 -> v0.1.1`, exit 1 |
| `scripts/release.sh bump minor` | `releasing v0.1.0 -> v0.1.1`, exit 1 |
| `scripts/release.sh bump major` | `releasing v0.1.0 -> v0.2.0`, exit 1 |
| `scripts/release.sh bump 0.1.0` | `releasing v0.1.0 -> v0.1.0`, exit 1 |
| `scripts/release.sh bump 0a1b2`, `scripts/release.sh patch`, `scripts/release.sh`, `scripts/release.sh bench linux` | usage, exit 2 |
| `scripts/release.sh bench`, `scripts/release.sh commit` | the Actions refusal, exit 1 |
| `VT_BENCH_MACHINE` unset: `scripts/release.sh bench` | `error: VT_BENCH_MACHINE must name the machine ...`, exit 1 |

The `M >= 1` rows are read from the code by the advisor (no 1.x version to run against).

**M7 sub-commands end to end (13g, coder).** In a throwaway clone (`git clone . .tmp/rel-clone`,
edited script and code copied in, committed there), with `GITHUB_ACTIONS=true`,
`GITHUB_STEP_SUMMARY=.tmp/summary.md`, `VT_BENCH_MACHINE=x`:

| Step | Expect |
|---|---|
| `bump patch` | gate green; HEAD `release: v0.1.1` touching only `Cargo.toml`, `Cargo.lock`; last line `bumped v0.1.1` |
| `RUNNER_OS=<host OS as GitHub names it> bench` | tests green; `perf/history/<host>/v0.1.1.json` with `"platform": "<host>"`; summary has one PASS table (self-compare) |
| `bench` again | refuses: record exists |
| `RUNNER_OS=Solaris bench` | refuses: unknown platform |
| copy the record into the two other platform folders, edit their `platform` field to match, then `commit` | three charts, titles name each platform; HEAD still one commit `release: v0.1.1` (`git log -2`); tag `v0.1.1`; `git diff --name-only HEAD~1 HEAD` lists Cargo files and `perf/` only |
| a stray edit outside `perf/` before `commit` | refuses, no amend, no tag |
| a `windows` record placed in `perf/history/linux/` next to a `linux` one, then `commit` | chart refusal, no tag |

M7 runs the real bench once (minutes); the copies stand in for the other runners. M6 is the real
three-OS check.

**M2 snapshot vs roadmap lists.** Every item named in a roadmap "Public:" line appears in
`public-api.unix.txt` (and the Windows-only `pty` items in the Windows file). Every root-level
type in the snapshot is in a roadmap list (`Snapshot*` covers `SnapshotRow`, `SnapshotCell`,
...); any other one gets a one-line reason in `acceptance.md` or is made private. Module-level
wildcards (`grid::*`, `intern::*`, `input::*`, `pty::*`) are satisfied by construction.

```bash
grep -E '^(#\[[^]]*\] )*pub (struct|enum|trait|type|fn|const) oneterm_vt::[A-Za-z0-9_]+([^:A-Za-z0-9_]|$)' \
    crates/vt/public-api.unix.txt \
  | sed -E 's/^(#\[[^]]*\] )*pub [a-z]+ oneterm_vt::([A-Za-z0-9_]+).*/\2/' | sort -u   # root items
```

**M3 drift is caught.** Add a dummy `pub fn` to `lib.rs`, run `scripts/public-api.sh check`:
diff shown, exit 1; revert; `check` exits 0. Run `update` once on the coder's OS and confirm
`git diff` is empty for both files (cross generation matches the committed files).

**M4 fuzz.** fuzz.md run for each target; record target, seconds, total execs, peak RSS, corpus
size, artifacts (must be none). A finding follows fuzz.md "A finding".

**M6 Release workflow dry run (owner, release.md step 7).** `dry_run` (default `true`) runs
`bump`, `bench` x3 and `finalize` and skips `publish`. Check: each `bench (<platform>)` summary
has its trip-wire table ending `PASS: 10 of 10 ...` with baseline `perf/history/<platform>/v0.1.0.json`
(first release: compared with itself); the `finalize` summary has the notes; artifact `release`
holds 8 files: `release.bundle`, `notes.md`, `chart-{linux,windows,macos}.svg`,
`v0.1.0-{linux,windows,macos}.json`, each record's `platform` matching its name; the three
`machine` strings name their runner; `git ls-remote --tags origin` shows no new tag and `main` on
the remote is unchanged. Nothing reached the remote, so the real run reuses the same version.
`act` is not used: it cannot run the Windows and macOS jobs.

**M5 docs.** `wc -l docs/user/*.md`: every file <= 120 lines except `api-map.md` (<= 170);
`for-agents.md` <= 100.

## `acceptance.md` (coder, then owner)

Sections: M1 table with observed output; M2 result and any reasons; M3 result; M4 table; M7 table;
release checklist steps 3-12 (release.md) with dates, sha, URLs and run ids, M6 run id (owner).
