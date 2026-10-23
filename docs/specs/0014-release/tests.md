# Tests and checks

## Automated (run by `cargo test --workspace` and CI)

**T1 + T2: `crates/vt/tests/user_guide.rs`** (integration test; reads files only, uses no crate
API; paths from `env!("CARGO_MANIFEST_DIR")`; plain `std`, no new dev-dependency):

| Test | Behaviour |
|---|---|
| `every_user_doc_is_a_doctest_module` | for every `docs/user/*.md`, `src/lib.rs` contains `include_str!("../../../docs/user/<file>")`; the failure lists the missing files |
| `api_map_names_every_public_type` | for both `public-api.{unix,windows}.txt`: collect module paths from `pub mod <path>` lines; for each line that, after leading `#[...] ` attributes, starts with `pub struct`, `pub enum`, `pub trait`, `pub type` or `pub union`, take the path (cut at the first `<`, `(`, ` `, `;`), keep it only when its parent is a module (skips associated types in impls), strip `oneterm_vt::`; `docs/user/api-map.md` must contain it in backticks (`` `grid::Screen` ``, `` `Terminal` ``); the failure lists every missing name |

Both are one assertion over a collected list, so one run shows every gap.

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

**M1 `release.sh` kinds.** The script prints `releasing vOLD -> vNEW` before its clean-tree
check, so with one uncommitted change each run prints the mapping, exits 1, modifies nothing:

| Run (workspace at `0.1.0`, dirty tree, `VT_BENCH_MACHINE=x`) | Prints |
|---|---|
| `scripts/release.sh patch` | `releasing v0.1.0 -> v0.1.1` |
| `scripts/release.sh minor` | `releasing v0.1.0 -> v0.1.1` |
| `scripts/release.sh major` | `releasing v0.1.0 -> v0.2.0` |
| `scripts/release.sh 0.1.0` | `releasing v0.1.0 -> v0.1.0` |
| `scripts/release.sh 0a1b2` | usage, exit 2 |

The `M >= 1` rows are read from the code by the advisor (no 1.x version to run against).

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

**M5 docs.** `wc -l docs/user/*.md`: every file <= 120 lines except `api-map.md` (<= 170);
`for-agents.md` <= 100.

## `acceptance.md` (coder, then owner)

Sections: M1 table with observed output; M2 result and any reasons; M3 result; M4 table;
release checklist steps 3-13 (release.md) with dates, sha, URLs and run ids (owner).
