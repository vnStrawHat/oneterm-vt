# Public-API snapshot

Two committed files hold the whole public surface of `oneterm-vt`, one line per item:

| File | Target it is generated for | Differs by |
|---|---|---|
| `crates/vt/public-api.unix.txt` | `x86_64-unknown-linux-gnu` | `pty::SignalMask`, `pty::Options::child_signal_mask` |
| `crates/vt/public-api.windows.txt` | `x86_64-pc-windows-msvc` | `pty::PipeReader`, `pty::PipeWriter`, `pty::Options::escape_args` |

macOS takes the Unix half of `pty` (the gating is `cfg(unix)`), so the Unix file covers it.

## Tool and pins (Decision 3)

| Pin | Value | Why |
|---|---|---|
| `cargo-public-api` | `0.52.0`, `cargo install --locked` | newest release (2026-05-25); no parameter names in output, so a rename is no drift |
| nightly | `nightly-2026-05-25` | rustdoc JSON needs nightly; 0.52.x needs `nightly-2025-11-22` or later, and the crate's `rust-version` needs a 1.96 or later compiler |
| flags | `-p oneterm-vt --all-features --simplified --color never --target <triple>` | below |

- `--all-features`: `regex` adds `search::SearchPattern::Regex`; the diagnostic features must
  add nothing, and an item they add by mistake shows up here.
- `--simplified` (once) omits blanket impls only. Auto-trait impls (`Send`, `Sync`, `Unpin`,
  unwind safety) stay: losing one breaks embedders. Derived impls stay for the same reason.
- If the pinned nightly fails to build rustdoc JSON, the coder walks back to the newest nightly
  that works (not older than the first nightly reporting 1.96, early March 2026) and changes the pin here and in
  the script in the same commit.

## `scripts/public-api.sh check|update`

Bash, `set -euo pipefail`, LF, executable, shellcheck clean; same conventions as `release.sh`.

```bash
NIGHTLY=nightly-2026-05-25
CARGO_PUBLIC_API=0.52.0
UNIX_TARGET=x86_64-unknown-linux-gnu
WINDOWS_TARGET=x86_64-pc-windows-msvc
```

| Step | Rule |
|---|---|
| args | exactly one of `check`, `update`; else usage on stderr, exit 2 |
| root | `cd "$(git rev-parse --show-toplevel)"`; `mkdir -p .tmp` |
| tool | `cargo public-api --version` output must contain `$CARGO_PUBLIC_API`; else `cargo install cargo-public-api --version "$CARGO_PUBLIC_API" --locked` |
| toolchain | `rustup toolchain install "$NIGHTLY" --profile minimal --target <targets this mode needs>` (idempotent) |
| host | `uname -s`: `MINGW*`, `MSYS*`, `CYGWIN*` -> `windows`; anything else -> `unix` |
| generate | `cargo +"$NIGHTLY" public-api -p oneterm-vt --all-features --simplified --color never --target "$T" > ".tmp/public-api.$os.txt"` |
| `check` | host file only: generate, then `diff -u "crates/vt/public-api.$os.txt" ".tmp/public-api.$os.txt"`; on difference print the diff, then `::error::public API changed; run scripts/public-api.sh update, commit both files, pick the bump in docs/user/versioning.md`, exit 1 |
| `update` | generate both targets (the non-host one via `--target`), move both into `crates/vt/`, print `git diff --stat -- crates/vt/public-api.*.txt` |

`check` uses the host target so CI never downloads a second standard library; `update` cross
generates so the owner refreshes both files from one machine (Decision 4). Rustdoc does not
link, so the cross build needs only `rustup`'s target component. If the cross-generated file
ever differs from a native one, the CI check on that OS prints the exact diff to apply.

The files are LF (`.gitattributes` forces it); the tool writes `\n` on every OS.

## CI (`.github/workflows/ci.yml`, job `check`)

```yaml
      - name: Public API snapshot
        if: runner.os != 'macOS'
        run: scripts/public-api.sh check
```

After "Test". `Swatinem/rust-cache` keeps `~/.cargo/bin`, so the tool builds once per cache key.
`shellcheck scripts/*.sh` already covers the new script.

## Update procedure (any change to the public surface)

1. Change the code, run `scripts/public-api.sh update`.
2. Read the diff of both files; pick the kind with the table below.
3. Commit the two files with the code; the commit type and `!` follow the kind
   (`docs/agents/commits.md`). CI fails until the files match.

## Snapshot diff -> version kind (copied into `versioning.md`)

| Diff of the snapshot files | Kind |
|---|---|
| no change | per behaviour rules only (reply bytes, encoder bytes, `FeedStats` meaning are major) |
| only added lines | minor, **except** the cases in the next row |
| added variant of an enum, or `pub` field of a struct (not `Config`, see versioning.md), whose declaration line lacks `#[non_exhaustive]` (the snapshot prints the attribute); added trait method without a default | major |
| any removed or changed line, including a lost `impl Send` / `Sync` / `Unpin` | major |

Under 0.x, major means `0.(x+1).0` and minor `0.x.(y+1)` (release.md). The release kind is the
highest kind among the commits since the last tag; `git diff v0.1.0 -- crates/vt/public-api.*.txt`
shows the whole surface change of a release at a glance.
