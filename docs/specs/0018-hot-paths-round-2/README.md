# 0018: Engine hot paths, second round

**Goal:** close the largest measured gaps of `perf/compare/i7-12700-win11/2026-10-29-2.md` in
`crates/vt`, in eight steps one coder can land one after another with the gate green after
each, then re-run the comparison. Every step was proven in a throwaway prototype (evidence.md).

**Non-goals:** new public items; event coalescing; the snapshot format; a faster width table;
changing the corpus, goldens or harness. What stays behind and why: end of evidence.md.

| File | Covers |
|---|---|
| [evidence.md](evidence.md) | method and tools, profiles and ablations, per-step prototype numbers, rejected experiments, verify result |
| [text.md](text.md) | S1 REP through the print run; S6 glyph runs (store-forwarding stall, edge-only pair release) |
| [styles-graphemes.md](styles-graphemes.md) | S2 background-only pen as erase style; S3 sweep sizing; S4 key-less cluster index |
| [parser.md](parser.md) | S5 CSI header run, short ESC look-ahead, direct `ESC [` entry |
| [scrolling.md](scrolling.md) | S7 rotated rows restamped once per batch; S8 in-place alternate-screen region |
| [tests.md](tests.md) | changed and new tests per step, chunk invariance, compare verify |
| [files.md](files.md) | step order, files per step, thresholds, gate, user docs, commits |
| `acceptance.md` | written by the coder: before/after per step, verify output, compare run |

## Acceptance criteria

- [ ] Steps S1-S8 landed in order (files.md), gate green after each, one commit each
- [ ] Every per-step threshold in files.md met (pinned `vt-bench grid`, median of 5, 32 MiB)
- [ ] No guard fixture below 0.95 x its value before the step; `vt-bench grid --check` passes
- [ ] `compare verify --engine oneterm`: the 21 golden fixtures still ok
- [ ] Public API snapshot unchanged (`scripts/public-api.sh check`, CI)
- [ ] `docs/user/events.md` updated in S8
- [ ] `compare run --machine i7-12700-win11` re-run and committed; `acceptance.md` filled

## Steps and expected gains (prototype, pinned MiB/s; best engine from the compare run)

| Step | Root cause (measured) | Fix | Target: before -> after | Best engine |
|---|---|---|---|---:|
| S1 | REP prints 100 copies one scalar at a time (70 % of `csi_mix`) | copies through the print run | `csi_mix` 56 -> 126 | 135 |
| S2 | two style lookups per SGR (`HashMap::get` 32 %) | a bg-only pen is its own erase style | `dense_cells` 190 -> 216 | 329 |
| S3 | sweep index rebuilt by doubling (rehash 19.5 %) | size it once | `combining_unique` 18.7 -> 22.0 | 152 |
| S4 | a boxed key per unique cluster (heap 35 %) | hash -> id map plus a chain in the spans | `combining_unique` 22.0 -> 48.7 | 152 |
| S5 | `memchr` per short run, `Escape` detour, per-byte CSI state (parser 40-60 %) | 8-byte look-ahead, direct `ESC [`, CSI header run (rio's technique) | `cursor_motion` 235 -> 308, `tui_redraw` 302 -> 381, `vim_session` 206 -> 256, `mode_churn` 153 -> 210 | 326, 484 |
| S6 | store-forwarding stall on `occ` per cell (`RowMut::set` 51 %) | glyph runs with edge-only pair release, one `occ` update; block-wise ASCII count | `cjk_wide` 215 -> 353, `unicode_symbols` 214 -> 352, `long_lines` 791 -> 1,036 | 403, 418 |
| S7 | every region row's header rewritten per LF (restamp loop 40 % of `scroll_region`) | restamp the moved range once per batch | `scroll_region` 343 -> 539 | 764 |
| S8 | alternate screen pushes and trims a row per region scroll | rotate the region in place (owner decision) | `scroll_region_small` 33.0 -> 58.5 | 79 |

HEAD -> S8 elsewhere: `heavy_sgr` 266 -> 306, `plain_ascii` 671 -> 776, `scrolling` 926 ->
1,230, `csi_mix` 56 -> 173; `short_lines`, `sixel`, `hyperlinks`, `long_osc` within noise.

## Decisions

1. 0017's deferred structural change pays for region scrolls in a narrow form (S7): ids stay
   positional; moved rows' headers are rewritten once per batch, not per scroll. It makes
   "a live slot holds nothing or its own row" load-bearing (checked by the integrity walk).
2. Clusters need no raw-entry table: a hash-to-id map plus a chain in the spans (S4).
3. No `unsafe`, no new dependency, no public API change in any step.

## Owner decision (open)

- **S8 changes what the alternate screen reports for a partial region at its top** (vim and
  less with a status line): one `RowsScrolled` over the region (delta -n) instead of
  `RowsScrolled` below it (+n) plus `RowsTrimmed`, and alternate row ids stop advancing on
  such scrolls. Counters keep their meaning. Gain: `scroll_region_small` 33.0 -> 58.5 MiB/s
  (best engine 79). Without S8 the other steps are unaffected; the spec lands it last.
