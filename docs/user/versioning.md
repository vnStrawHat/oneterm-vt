# Versioning

Releases are git tags `vX.Y.Z`; consumers depend by tag. Standard semver, three kinds only.

```toml
[dependencies]
oneterm-vt = { git = "<your fork url>", tag = "v0.1.0" }
```

| Kind | When |
|------|------|
| `major` | Breaking change: a removed or renamed public item, a signature change, a new variant on an exhaustive enum, a change to the reply bytes (DA1-3, DSR, DECRQM, XTVERSION, DECRQSS, XTGETTCAP, DECRQCRA, OSC colour queries) or to the input encoders' output, an MSRV raise. The commit carries `!` / `BREAKING CHANGE:`. |
| `minor` | New public item, module, default-off feature, new variant on a `#[non_exhaustive]` enum, new behaviour (`feat`). |
| `patch` | `fix`, `perf`, internal changes with no public effect. |

Details:

- `parser::Dispatch` is an engine-internal contract for advanced use; changes to its method set
  are a minor bump.
- Items behind a non-default feature carry the same promise as the default surface.
- Reply bytes are API: programs parse them. `Config::product_name` changes the identity part of a
  reply, never its shape.
- The meaning of each `FeedStats` counter is API; counting something different is a major bump.
- `polling` is the one public dependency (under `pty`); a `polling` major bump is a `major` bump here.
- `Config` is built as `Config { field: value, ..Config::default() }`; a new `Config` field is a minor change.
- MSRV: 1.96.0 (`rust-version`); raising it is a major change.

Not promised: grid internals reachable through `grid::Screen`, exact `SeqNo` values, allocation
behaviour, performance. Promised: a `RowId` names the same content for as long as that content is
live, because embedder caches key on it.

## Before 1.0

While the crate is `0.x` these rules still apply literally, so embedders can read the number the
same way before and after 1.0: a major change goes `0.x.y` to `0.(x+1).0`; a minor or patch change
goes `0.x.y` to `0.x.(y+1)`. `scripts/release.sh` applies this mapping.

## The public-API snapshot

Two committed files hold the whole public surface, one line per item: `crates/vt/public-api.unix.txt`
(Linux and macOS) and `crates/vt/public-api.windows.txt`. CI fails when the code and the files
disagree. Refresh both with `scripts/public-api.sh update`, read the diff, and pick the kind:

| Diff of the snapshot files | Kind |
|---|---|
| no change | per the behaviour rules above only |
| only added lines | minor, except the next two rows |
| added variant of an enum, or `pub` field of a struct (not `Config`), whose declaration line lacks `#[non_exhaustive]`; added trait method without a default | major |
| any removed or changed line, including a lost `impl Send` / `Sync` / `Unpin` | major |

The behaviour-only rules above (reply bytes, encoder bytes, `FeedStats` meaning) are not visible
in the snapshot and still apply. `git diff v0.1.0 -- crates/vt/public-api.*.txt` shows the whole
surface change since a release.

## How a release is cut

Only the manual Release workflow (`.github/workflows/release.yml`) releases. It runs
`scripts/release.sh` on a hosted Linux runner (bump, gate, benchmark record, chart, commit
`release: vX.Y.Z`, tag), tests that exact commit on Linux, Windows and macOS, then pushes commit
and tag and creates the GitHub release with the chart and the record attached. A tag is never
moved; a broken release is followed by a new patch release.

## Release notes

Generated from commits by the Release workflow; see [docs/agents/commits.md](../agents/commits.md).
