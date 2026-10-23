# 0002: Parser (phase 1)

## Goal

`oneterm_vt::parser`: a VT500-style byte state machine that turns an untrusted byte stream into
`Dispatch` calls, with bounded memory, UTF-8 decoding with replacement, and a `memchr` ground-state
fast path. Plus the 10 bench fixtures and `vt-bench parser` (tier 1) and `vt-bench fixtures`.

## Non-goals

- Acting on sequences (phase 6) or decoding Sixel (phase 10); the parser knows no grid or mode.
- Counters (`FeedStats` is phase 6). The parser makes every dropped *sequence* observable to the
  sink through `ignore`, `truncated` and `aborted`; see [state-machine.md](state-machine.md).
- 8-bit C1 introducers (`0x9B` as CSI): C1 is executed, never an introducer (D1).

## Files in this spec

| File | Covers |
|---|---|
| [state-machine.md](state-machine.md) | states, transition tables, ground fast path, UTF-8, C1, strings, drops |
| [params.md](params.md) | `Params`, `ParamSep`, `ParamGroups`, `Intermediates`, `OscParams`, buffers and caps |
| [dispatch.md](dispatch.md) | `Dispatch` trait, call ordering, `Parser` API, chunk invariance |
| [bench-fixtures.md](bench-fixtures.md) | 10 fixture generators, tier-1 `parser`, `fixtures --out`, output formats |
| [tests.md](tests.md) | unit, proptest, 1M-byte, `tests/parser_limits.rs`, user chapter `parser.md` |
| [files.md](files.md) | files to create/edit, dependencies |

## Public API (fixed by the roadmap)

`oneterm_vt::parser::{Parser, Dispatch, Params, ParamGroups, ParamSep, Intermediates, OscParams,
StringTerm, MAX_PARAMS, MAX_INTERMEDIATES, MAX_OSC_PARAMS, OSC_INLINE, OSC_LARGE, DCS_MAX_BYTES}`.
Nothing else is `pub`. `Parser` methods: `new`, `advance`, `reset` (+ `Default`).

## Acceptance criteria

- [ ] Quality gate (fmt, clippy `-D warnings`, `cargo test --workspace`) passes
- [ ] `cargo test -p oneterm-vt --no-default-features` passes
- [ ] `RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps --all-features` passes
- [ ] Every test in [tests.md](tests.md) exists and passes, including the proptest and the 1M-byte test
- [ ] `docs/user/parser.md` exists; its Rust blocks run under `cargo test --doc`
- [ ] Memory caps hold: OSC buffer capacity never exceeds `OSC_LARGE` (claimed) / `OSC_INLINE`
      (unclaimed); DCS/APC puts stop at `DCS_MAX_BYTES` with exactly one aborted end
- [ ] No `unsafe`, no `unwrap`/`expect`/`panic!` in non-test code; no `pub` item outside the list
- [ ] `cargo run -p oneterm-vt-bench --release -- parser` prints the tier-1 table;
      `parser --json --machine X` prints a record matching docs/perf schema (tier 1 only)
- [ ] `vt-bench fixtures --out .tmp/fx --mib 1` writes 10 `.vt` files
- [ ] Tier-1 numbers recorded in [bench-fixtures.md](bench-fixtures.md) "Acceptance notes"

## Decisions worth a second look

1. Ground scan uses `memchr` on `ESC` only, not `memchr3`; reason in state-machine.md.
2. Malformed CSI/DCS headers still dispatch once with `ignore = true` (Williams drops them
   silently), so phase 6 can count them.
3. OSC cancelled by `CAN`/`SUB` is discarded, not dispatched; `C1 ST` (`0x9C`) never terminates.
4. All `Dispatch` methods except `print_str` and `execute` have no-op defaults.

## Open items (owner)

1. Confirm decisions 1-4 above.
2. `--json` requires `--machine` (perf schema says required). Confirm, or allow a placeholder.
