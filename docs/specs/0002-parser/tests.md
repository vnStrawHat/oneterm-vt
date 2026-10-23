# Tests

Unit tests live in `parser/parser_tests.rs` (`#[cfg(test)] mod parser_tests;` in `mod.rs`), except
the OSC capacity tests, which sit in `osc.rs` next to the private buffer.

Helpers (test-only): `enum Action` (one variant per `Dispatch` call; prints as `Print(char)`,
`Csi { groups: Vec<Vec<u16>>, intermediates: Vec<u8>, ignore, byte }`, `Osc { code, params:
Vec<Vec<u8>>, term, truncated }`, ...), `Recorder` (pushes `Action`s; `claims: Vec<u32>` answers
`osc_allows_large`), `feed(bytes) -> Vec<Action>`, `feed_chunked(bytes, size)`, and
`feed_both_ways(bytes)` = asserts whole == 1-byte chunks, returns the log.

## Per state (all through `feed_both_ways`)

| Test | Input -> expected log |
|---|---|
| `ground_prints_and_executes` | `ab BEL DEL` -> Print a, b, Execute 07, Execute 7F |
| `escape_executes_c0_and_stays` | `ESC BEL A` -> Execute 07, Esc([], A) |
| `escape_is_idempotent` | `ESC ESC ESC [ A` -> one Csi [[0]] `A` |
| `can_sub_cancel_every_header_state` | for prefixes `ESC`, `ESC (`, `ESC [`, `ESC [1`, `ESC [ SP`, `ESC [1?`, `ESC P`, `ESC P1`, `ESC P SP`, `ESC P1?`, each + CAN/SUB + `A` -> Execute 18/1A, Print A |
| `escape_intermediates` | `ESC # 8` -> Esc([#], 8); `ESC ( DEL B` -> Esc([(], B) |
| `escape_third_intermediate_sets_ignore` | `ESC SP ! # 8` -> Esc([SP, !], ignore, 8) |
| `csi_entry_forms` | `CSI m`, `CSI SP q`, `CSI ? 25 h` per params.md |
| `csi_param_after_intermediate_dispatches_ignored` | `CSI SP 1 m` -> Csi ignore=true (D2) |
| `csi_marker_after_param_dispatches_ignored` | `CSI 1 ? m` -> Csi [[1]] ignore; `CSI ?1;2? BEL m` -> Execute 07, Csi ignore |
| `csi_drops_high_bytes` | `CSI 1 0x9B m` -> Csi [[1]] `m` |
| `dcs_header_drops_c0` | `ESC P BEL q ESC \` -> Hook([[0]], q), Unhook(false), Esc([], `\`) |
| `dcs_streams_payload` | `ESC P 1;2 q #0~ BEL ESC \` -> Hook([[1],[2]]), Put per byte incl. 07, Unhook(false), Esc |
| `dcs_drops_high_bytes_and_c1_st` | `ESC P q a 0x9C b ESC \` -> Put a, Put b, Unhook(false), Esc |
| `dcs_malformed_header_hooks_ignored` | `ESC P 1 ? q xy ESC \` -> Hook ignore=true, Put x, y, Unhook(false), Esc |
| `dcs_can_aborts` | `ESC P q ab CAN` -> Hook, Put a, b, Unhook(true), Execute 18 |
| `apc_sos_pm_share_a_state` | `ESC _ Gi=1 ESC \`; `ESC X ! CAN`; `ESC ^ x BEL ESC \` (BEL is payload) |
| `osc_bel_and_st` | `ESC ] 0;hi BEL` -> Osc(Some 0, [0, hi], Bel); `ESC ] 2;t ESC \` -> Osc(St), Esc `\` |
| `osc_keeps_utf8_and_c1_payload` | `ESC ] 2; C3 A9 9C BEL` -> param 1 = `C3 A9 9C` |
| `osc_drops_c0` | `ESC ] 0;a 01 b BEL` -> param 1 = `ab` |
| `osc_cancelled_by_can_is_not_dispatched` | `ESC ] 0;x CAN` -> Execute 18 only |
| `osc_code_parsing` | `ESC ] BEL` -> None, 1 empty param; `52;c;Zm9v` -> Some 52; `x1;y` -> None; `4294967295` -> Some(u32::MAX); `4294967296;a` -> None |
| `osc_params_past_limit_join_into_last` | params `0;1;...;19` -> 16 params, last = `15;16;17;18;19` |
| `osc_allows_large_asked_once_per_osc` | counting sink: `52;c;x BEL` -> 1 call (52); `ESC ] BEL` and `x;y` -> 0 calls |

## Params, intermediates, UTF-8

| Test | Covers |
|---|---|
| `params_table` | every row of the table in params.md |
| `params_seps_kept` | `CSI 38:2::1:2:3;4 m`: `sep(i)` = S,C,C,C,C,C,S |
| `params_overflow` | 32 values -> not ignored; 33 values or 32 + trailing `;` -> first 32 kept, ignored |
| `param_saturates_at_u16_max` | `CSI 99999999999 m` -> [[65535]], not ignored |
| `c1_is_executed_never_an_introducer` | raw `80 90 9B 9C 9D 9F a` -> Executes + Print a; `C2 9B [ A` -> Execute 9B, Print `[`, `A` |
| `utf8_cases` | `a EF BC b` -> a, FFFD, b; `E0 80` -> FFFD, Execute 80; `F0 9F 8E 89 _` -> one run; `D8 ESC 0 1 2` -> FFFD, Esc `0`, 1, 2 |
| `utf8_carry_cases` | split at `|`, compared with the unsplit log and an explicit one: `F0 9F | 9A 80`; `C4 | B8 F0 9F 8E 89`; `E4 BF | 99 B5` -> U+4FD9, FFFD; `C0 C5 | 93 40 97` -> FFFD, U+0153, `@`, Execute 97; `E4 | ESC 7` -> FFFD, Esc `7` |
| `print_runs_are_batched` | 4096 x `x` -> one `print_str`; `ab LF cd CR LF ef` -> runs ab, cd, ef; `ab TAB cd` -> ab, cd |
| `invalid_run_stays_linear` | 1 MiB of `0xFF`, no ESC -> 1 Mi replacement prints, finishes (counting sink) |
| `reset_returns_to_ground` | mid-OSC `reset` then `hi` -> Print h, i only; carry `E4`, `reset`, `a` -> Print a only |
| `parser_fixed_size` | `const _: () = assert!(size_of::<Parser>() <= 512);` |

## Memory caps (unit, `osc.rs`, reach the private buffer)

- `unclaimed_osc_never_grows`: 20 MiB unterminated `ESC ] 0;` + `A`s in 64 KiB chunks ->
  `payload.capacity() == OSC_INLINE` throughout, `truncated`.
- `claimed_osc_grows_to_large_and_shrinks_back`: claim 52, 9 MiB payload -> stored len
  `OSC_LARGE`, capacity `<= OSC_LARGE`, `truncated`; after BEL -> capacity `== OSC_INLINE`.

## Integration: `crates/vt/tests/parser_limits.rs` (public API only, no allocator hooks)

| Test | Assert |
|---|---|
| `dcs_payload_stops_at_cap_and_aborts_once` | `ESC P q` + `DCS_MAX_BYTES + 1000` bytes + `ESC \` -> puts `== DCS_MAX_BYTES`, ends 1, aborted 1 |
| `dcs_payload_at_cap_is_not_aborted` | exactly `DCS_MAX_BYTES` bytes + `ESC \` -> one end, not aborted |
| `apc_payload_stops_at_cap_and_aborts_once` | same as the DCS case with `ESC _ G` |
| `osc_unclaimed_is_cut_at_inline` | `ESC ] 0;` + 1 MiB + BEL -> sum of param lengths `== OSC_INLINE`, truncated |
| `osc_claimed_is_cut_at_large` | claim 52; `ESC ] 52;c;` + 9 MiB + BEL -> sum `== OSC_LARGE`, truncated |
| `osc_claim_is_per_code` | claim 52; `ESC ] 0;` + 9 MiB + BEL -> sum `== OSC_INLINE` |
| `params_and_intermediates_stay_bounded` | 10 000 `;1` values and 100 intermediates -> `params.len() == MAX_PARAMS`, intermediates len `== MAX_INTERMEDIATES`, ignore |

The live-heap measurement (counting `#[global_allocator]`) is not done here: `unsafe_code` is
denied outside `pty` and `vt-bench`. The OSC buffer is the parser's only heap allocation, so the
two capacity tests above bound the heap; tier 5 (phase 6) measures live heap in `vt-bench`.

## Chunk invariance and hostile input (`mod props` in `parser_tests.rs`)

- `chunking_is_invariant` (proptest, default 256 cases): `bytes` = vec of 0..2048 from
  `prop_oneof![any::<u8>(), select(BIAS)]`, `sizes` = vec of `1..=64`, len `1..32`, cycled as
  chunk sizes. Log of chunked feed == log of whole feed, with a `Recorder` claiming 52.
  `BIAS` = ``b"\x1b[]P_X^;:?<>0123456789m\x07\x18\x1a\\\xc2\x9b\x9c\xe4\xf0"``.
- `million_random_bytes_never_panic`: xorshift64 (seed `0x5EED_0001`), 1 MiB with every third
  byte replaced from `BIAS`, fed in 8 KiB chunks to a counting sink (no log stored). Then a
  64 KiB sample: whole log == log for chunk sizes `1, 2, 3, 7, 997, 4096`.

## User chapter `docs/user/parser.md` (row 14 in docs/user/README.md)

Coder writes it (<= 120 lines, rules in docs/user/README.md). Every Rust block is a doctest:

1. Example first: a `StripEscapes` sink (`print_str` appends, `execute(b'\n')` appends `\n`) fed
   `"\x1b[1;31mred\x1b[0m plain\r\n"` in two chunks split inside a CSI; `assert_eq!` on
   `"red plain\n"`.
2. Reading SGR groups: `csi` collecting `params.groups()` for `ESC[38:2::255:0:0m` -> `[[38, 2, 0,
   255, 0, 0]]`, and the `;` form for contrast.
3. An OSC title sink: `osc` with `code == Some(2)` -> `params.get(1)`.
4. Checklist of what the sink owes: drop `ignore == true`; pure `osc_allows_large`; `ESC \` arrives
   as `esc`; DCS/APC stream byte by byte; slices live only during the call; `reset` calls nothing.
5. Limits table: the six constants and what happens past each.

Wire it in `lib.rs`: `#[cfg(doctest)] #[doc = include_str!("../../../docs/user/parser.md")]
mod user_guide_parser {}`.
