# Tests

Helper (test-only, `terminal/terminal_tests.rs`): `Session { term, batch }` with `new(cols,
rows)`, `with(Config)`, `feed(&[u8]) -> FeedStats`, `replies() -> String` (all `Reply` bytes of
the last feed), `row(index) -> String` (`row_text` of screen row `index`), `cursor() -> (u16,
u16)` (screen index, col), `style_at(index, col) -> Style`.

## Unit: `events/event_tests.rs`

| Test | Assert |
|---|---|
| `events_in_byte_order_repaint_last` | `ESC]2;a BEL`, `BEL`, `ESC[c` -> `Title`, `Bell`, `Reply`, `Repaint` |
| `repaint_once_and_only_when_dispatched` | two prints -> one `Repaint`; a lone `ESC` (incomplete) -> no events |
| `spans_read_back_and_never_panic` | spans read back; out-of-range spans -> `""` / `&[]` / empty iterator |
| `non_utf8_text_is_dropped_at_insert` | `push_text(b"\xff", ..)` -> `false`, no event, arena unchanged |
| `finish_trimmed_rewinds_on_invalid_utf8` | arena length back to `mark` |
| `clear_shrinks_past_the_soft_caps` | 2 MiB arena / 10,000 events -> after `clear` capacity `<= 1 MiB` / `<= 4096` |
| `steady_feed_keeps_capacities` | 1,000 feeds of `line\r\n` + `ESC]0;t BEL`: all three capacities constant after the first 10 |
| `scroll_report_maps_to_events` | a report with both fields -> `RowsScrolled`, then `RowsTrimmed` |
| `public_push_methods_build_a_batch` | `push_title`, `push_clipboard_store` (UTF-8 and not), `push_reply`, `push_osc(9, &[b"9", b"x"], ..)`, `push` + `push_repaint` twice -> exactly the expected events, one `Repaint` |
| `forwarded_osc_resplits_the_16th_param` | `Forward` route, `OSC 7501` with 20 `;`-separated params -> 21 spans in order |

## Unit: `terminal/terminal_tests.rs` (one behaviour per test)

- Print: ASCII prints and wraps (`WRAPPED`); `ESC ) 0 SO qqq SI qqq` -> `───qqq`;
  `ESC ( 0 lqk ESC ( B lqk` -> `┌─┐lqk`; DECSC/DECRC restore designations and shifts; SS2/SS3
  shift exactly one printed char and survive an intervening `CSI m`; LS2/LS3 persist.
- REP: `ab CSI 4 b` at 4 cols -> `abbb` / `bb`; the source survives `CSI 32 m`; nothing printed
  yet -> no-op.
- `?2027`: a ZWJ family split across two feeds lands in one cell; a control between the halves
  breaks it; a 40-scalar cluster -> `dropped_cluster_carries 1` and no carry; a lone VS16 is
  width 0; `resize` drops the carry.
- C0: BEL event; BS at col 0 keeps pending wrap; `?45` crosses only into a `WRAPPED` row and
  never above the region top; HT with pending wrap; VT/FF act as LF; DEL prints nothing, uncounted.
- CSI: every cursor row of dispatch-csi.md with and without its parameter (`CSI 0 A` = `CSI A`);
  CUU/CUD stop at the margins inside the region, at the screen edge outside it; under DECOM CUU
  never moves down and CHA never changes the row; CUP/VPA region-relative; CHT/CBT keep pending
  wrap; ICH/DCH/ECH/EL on `abcdef` with and without counts.
- DECSTBM: valid -> set and home; `CSI 5;3 r`, `CSI 3;3 r`, `CSI 30;40 r` (24 rows) leave region
  and cursor unchanged, uncounted.
- SGR: every row of the SGR table; `38;2;300;1;2` -> bold + dim, fg default; `21` -> double
  underline; `4:0` / `24` clear; unknown groups skipped without losing the rest.
- Scroll events: ED 2 on the primary scrolls into history after `ScreenCleared`; ED 3 pushes it
  with empty history; SU/SD/IL/DL push `RowsScrolled`; a wrap at the bottom of a partial region
  pushes `RowsScrolled`; a wrap that trims history pushes `RowsTrimmed` and counts `rows_scrolled`.
- Selection: ED/EL/DECALN/DECCOLM/real screen switch/RIS ask `invalidated_by` **before** the op;
  a dead selection is released after `feed` and `resize`; a fresh empty click survives output.
- Modes: DECRQM walk over every private and ANSI mode at power-on and after `h` (inert never
  `Set`); mouse reporting and encoding exclusive on `h`, not on `l`; DECOM homes on set and on
  reset; DECCOLM keeps the width; `?5` changes no cell; `?2026` is stored.
- Alt screens: 47/1047/1049 enter and leave; DECSC before `?47 h` survives `?47 l` + DECRC; 1049
  restores the primary cursor; 1047 wipes on leave; keyboard stacks swap; switches add no line.
- Resets: RIS clears modes, title stack, charsets, overrides, hyperlink and extras tables, keeps
  row ids and `lines_produced`; DECSTR resets and keeps exactly what modes.md lists.
- Cursor style: DECSCUSR 0-6; `CSI 9 SP q` unhandled; `?25 l` -> `Hidden` with shape kept; `?12`;
  `CursorStyleChanged` exactly when `cursor_style()` changed (incl. `?25`), never otherwise.
- Kitty and `modifyOtherKeys`: every rule of modes.md; pop `>= len` empties; 12 pushes keep 8.
- `lines_produced`: wraps 0, `CRLF CRLF` 2, IND + NEL 2, `?1049 h` + `l` 0, `CSI 3 S` 3.
- Grapheme sweep: trigger reached -> dead ids gone, live cells resolve to the same text, swept
  rows carry the batch stamp, interner counters carried over.
- `set_scrollback_limit`: lower trims history (anchors die, selection pruned), higher re-rings;
  surviving row ids and texts unchanged.
- `hit_test` and the seven selection wrappers per spec 0006 (escape set from `Config`).
- `Terminal: Send + Sync` (const assertion).

## Unit: `terminal/osc_tests.rs`, `terminal/query_tests.rs`

- Routes: defaults; each route per osc.md (typed event before `Osc`); canonical equality;
  `Builtin` on a non-builtin reads `Drop`; `large` per number and never for a `Drop` route; spill
  numbers above 2047; `route_all` = `route` per code; `overrides()` lists exactly the non-default
  numbers, ascending, spill included.
- Every row of osc.md and colors.md: accept and each reject (counted once); zero-padded codes;
  OSC 4 with 12 pairs (16th param re-split); OSC 7 with `;` in the path; title stack cap 16.
- Base64: `Zm9v`, `Zm8=`, `Zg==`, empty -> stored; `Zm9`, `Zm=v`, `Z===`, `Zg==Zg==`, `Zm9v!`,
  `/w==` (byte `FF`, not UTF-8) -> `malformed_sequences`.
- Colours: `ColorKey::from_index(k.index()) == Some(k)` for every key; `from_index(n) == None` for every `n` in `259..=268` and `>= 271`;
  `iter()` yields `(index, rgb)` ascending; OSC 9 `Notification` has an empty `title`.
- DECRQSS `SP q` with `default_cursor_style` `HollowBlock` -> `2 SP q` (steady), blinking -> `1`.
- Queries: buffer reuse over 10,000 XTGETTCAP requests (capacity constant); payload capacity
  stays `<= 8 KiB` after a 9 KiB request; aborted query answers nothing; RIS/DECSTR clear the
  buffer; DECRQSS SGR round-trip: 15 SGR states replayed into a second terminal give equal styles.

## Integration (public API only)

| File | Content |
|---|---|
| `tests/terminal_replies.rs` | **one byte-exact test per row** of the queries.md reply table; plus DA2 with `Acme(1.4.0)` and the hostile versions; XTVERSION sanitising (`Ev\x1b\\il` -> `Ev\\il`, 64-byte cap, a 4-byte char straddling it dropped whole, control-only -> engine name); DECRQCRA (gate shut: unhandled, no reply; `A` -> `0041`; blank -> `0020`; attributes ignored; `e` + `U+0301` -> `0366`; DECOM clamp; scrollback unreachable); every DECRQSS request and refusal; XTGETTCAP `TN` default and product, `Co`, `RGB`, three names in order, non-hex echo empty; both `color_reply` examples |
| `tests/feed_stats.rs` | **one test per `FeedStats` field** (12), each asserting the exact value its minimal stream must give, plus "per call": a following `feed(b"a")` reads 0. Streams: `bytes` `abc`; `rows_scrolled` 5 LF on 3 rows -> 3; `unhandled_sequences` `CSI ? 9999 z` -> 1; `malformed_sequences` `OSC 52;c;!!!!` -> 1; `truncated_osc` 3 KiB OSC 0 -> 1; `aborted_dcs` `DCS + q` + `DCS_MAX_BYTES + 1` bytes + ST -> 1; `hyperlink_table_exhausted` 65,537 distinct links -> >= 1; `style_table_exhausted` 65,536 distinct RGB fg (one char each) -> 1; `grapheme_truncated` `e` + 16 x `U+0301` -> 1; `grapheme_table_exhausted` 270,000 distinct live clusters (160 cols, 10,000 rows of scrollback) -> > 0; `dropped_cluster_carries` `?2027 h` + `e` + 40 marks -> 1; `wide_glyphs_dropped` `中` on 1 column -> 1 |
| `tests/feed_limits.rs` | every hostile-stream row of limits.md not marked as a unit test |
| `tests/feed_chunking.rs` | proptest (256 cases): bytes from a biased alphabet (`ESC [ ] P ; ? 0-9 m H J K r BEL CAN a 中 U+0301`), chunked by random sizes `1..=64`: same screen text, cursor, title and the same non-`Repaint` events (payloads resolved) as one whole feed |

No allocator hook in `crates/vt` (`unsafe` stays out): `steady_feed_keeps_capacities` plus the
grid's address-stability tests (spec 0004) cover the steady state; tier 5 measures live heap.

## User chapters (coder; <= 120 lines each; every Rust block a doctest, wired in `lib.rs`)

| Chapter | Outline |
|---|---|
| `docs/user/events.md` (03) | example first: feed `ESC]2;hi BEL` + DA1, drain `Title` and `Reply`; every `VtEvent` and what the embedder owes; answering `ColorQuery` with `color_reply`; `ClipboardLoad` policy; OSC routes: a forwarded private number and `BuiltinAndForward` |
| `docs/user/limits.md` (08) | the ceiling table of limits.md; a per-call `FeedStats` example; each counter's meaning; what the embedder still owns |
| `docs/user/conformance.md` (09) | supported C0/ESC/CSI/modes/OSC/DCS; inert modes and their DECRQM answers (doctest); the DECRQCRA gate; known gaps: DECSLRM, DECSCA/DECSED/DECSEL, `CSI * x`, DA1 level 62, 8-bit C1 introducers |
