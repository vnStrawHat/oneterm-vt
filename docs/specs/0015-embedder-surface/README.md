# 0015: Embedder surface (post-0.1)

**Goal:** close the gaps a full embedder hits when it builds on the engine: mutable grid and
intern access for fixtures and viewport control, root names for every intern type a public
signature hands out, an exhaustive `ColorKey`, and `parser::Dispatch` passing intermediates as a
plain `&[u8]`. Every other difference found by the audit is kept and documented, with the
one-line caller change.

**Non-goals:** a test-only feature or builder module (the two accessors are enough); raw cell
writes that bypass wide-pair repair; `ColorKey` variants no sequence reaches; changing grapheme
storage from UTF-8; single-row search matches; changing reply bytes (`CSI 14 t`, SGR `21`).

| File | Covers |
|---|---|
| [gaps.md](gaps.md) | the audit: counts per class, identical items by area, G1-G14 with decisions |
| [api.md](api.md) | A1-A4 signatures and rustdoc contract; what is kept and why |
| [tests.md](tests.md) | unit U1-U6, parser P1-P2, integration `tests/embedder_surface.rs` S1-S6, doctests, gate |
| [files.md](files.md) | every file touched, docs, public-API snapshots, version kind, commit |

## Acceptance criteria

- [ ] G1-G5 implemented as api.md says; G6-G14 unchanged in behaviour
- [ ] U1-U6, P1-P2, S1-S6 pass; the new doctests in `grid.md`, `parser.md`, `search.md` pass
- [ ] `tests/embedder_surface.rs` exercises every call shape in gaps.md G1-G6 and G8-G12 through
      public paths only: the listed embedder call sites compile against the new surface with no
      change other than those in the G6-G14 "Decision" column
- [ ] Quality gate (fmt, clippy `--all-features -D warnings`, `cargo test --workspace`),
      `cargo test -p oneterm-vt --no-default-features`, docs with `-D warnings`
- [ ] `fuzz/` builds; `vt-bench` builds and its tests pass
- [ ] Public-API snapshots refreshed; their diff shows exactly: `Terminal::grid_mut`,
      `Terminal::interner_mut`, six root re-exports, `ColorKey` without `#[non_exhaustive]`, three
      `Dispatch` signatures, `parser::Intermediates` gone
- [ ] Commit per files.md (major under 0.x: `0.2.0`)

## Decisions

1. Both accessors are public, documented and ungated: a fixture, a replay tool and a viewport
   controller are all legitimate callers, and a feature flag would split the API by build.
2. Writes through `grid_mut` keep every grid invariant (`row_mut` range check, wide-pair repair);
   what they bypass is the parser and the terminal-level state, and the rustdoc says so.
3. An intern type gets a root name when embedders keep it in their own types or build it by hand (api.md A2).
4. `ColorKey` is exhaustive: a query answered from a theme needs a default per key, so a new key
   must break the build, not fall to "no reply".
5. `Dispatch` uses `&[u8]`: the wrapper type added nothing a slice does not give, and slice
   patterns are how every sink matches intermediates.

## Design choices with alternatives (A = as proposed, B = the simpler alternative)

| # | Proposed (A) | Simpler alternative (B) | Evidence |
|---|---|---|---|
| D1 | `grid_mut` / `interner_mut` | a `GridWriter` test builder in the engine | the audit's fixture needs five operations, all already public on `TerminalGrid`/`Screen`/`Interner`; B duplicates them |
| D2 | `&[u8]` intermediates, `Intermediates` private | keep `&Intermediates`; callers write `.as_slice()` | B leaves a public type whose only method is `as_slice`; A is the shape sinks already match on |
| D3 | `ColorKey` exhaustive | keep `#[non_exhaustive]`; callers add `_ => None` | a wildcard silently answers "no reply" for a future key |
| D4 | keep `Option<RowMut>` from `row_mut` | return `RowMut` and materialise any id | B lets a stale id overwrite the live row sharing its ring slot |

Rows are independent: B on a row reverts only that row's rules and tests.

## Open items

None.
