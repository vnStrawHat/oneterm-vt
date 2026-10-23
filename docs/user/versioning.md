# Versioning

Releases are git tags `vX.Y.Z`; consumers depend by tag. Standard semver, three kinds only.

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

Not promised: grid internals reachable through `grid::Screen`, exact `SeqNo` values, allocation
behaviour, performance. Promised: a `RowId` names the same content for as long as that content is
live, because embedder caches key on it.

## Before 1.0

While the crate is `0.x` these rules still apply literally (a major change goes `0.x` to
`0.(x+1)`), so embedders can read the number the same way before and after 1.0.

## Release notes

Generated from commits at tag time; see [docs/agents/commits.md](../agents/commits.md).
