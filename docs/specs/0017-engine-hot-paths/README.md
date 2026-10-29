# 0017: Engine hot paths after the first cross-engine run

**Goal:** fix the one correctness bug (`csi_mix` style table) and the slow paths that the
first cross-engine run (`perf/compare/i7-12700-win11/2026-10-29.md`) exposed in `crates/vt`,
in eight steps one coder can land one after another with the gate green after each, then
re-run the comparison.

**Non-goals:** new public items; event coalescing (breaks "same events for any chunking");
changing the row-id scheme; SGR/CSI parsing speed (`csi_mix`, `heavy_sgr`, `dense_cells`);
Sixel; changing the corpus, the goldens or the compare harness.

| File | Covers |
|---|---|
| [evidence.md](evidence.md) | baseline numbers, profiles, prototype A/B per step, prototype verify result |
| [styles.md](styles.md) | item 1: style-id reclamation (`csi_mix` diverges) |
| [text-path.md](text-path.md) | items 4 and 6: parser ASCII scan, bulk ASCII cell writes, wide glyphs in the run |
| [scrolling.md](scrolling.md) | items 3 and 5: slice rotation of region rows, event-list soft cap, row recycling |
| [graphemes.md](graphemes.md) | items 2 and 7: grapheme sweep pacing and loss, one intern per mark run, `?2027` path |
| [tests.md](tests.md) | regression tests per step, chunk invariance, compare verify |
| [files.md](files.md) | step order, files per step, per-step acceptance numbers, gate, commits |
| `acceptance.md` | written by the coder: before/after numbers per step, verify output, compare run |

## Acceptance criteria

- [ ] Steps S1-S8 landed in order (files.md), gate green after each, one commit each
- [ ] Every per-step threshold in files.md met (pinned `vt-bench grid`, median of 5, 32 MiB)
- [ ] No fixture below 0.95 x its value before the step (pinned); `vt-bench grid --check` passes
- [ ] `compare verify --engine oneterm`: `csi_mix` ok, the other 20 golden fixtures still ok
- [ ] Public API snapshot unchanged (`scripts/public-api.sh check`, CI)
- [ ] User docs updated in the step that changes the behaviour (files.md)
- [ ] `compare run --machine i7-12700-win11` re-run and committed; `acceptance.md` filled

## Decisions

1. Style ids are reclaimed by the extras mechanism (free list, no renumbering), not by a rebuild
   plus remap: no row is rewritten or re-stamped, snapshots never hold a `StyleId` (styles.md).
2. A style sweep runs only on a style miss with fewer than 2 free slots; hits never check fill.
3. The grapheme sweep keeps at most half of each arena cap, newest rows first; a dropped live
   cluster becomes its first scalar, a refused one leaves the cell as it was (graphemes.md).
4. `EventBatch` keeps up to 32,768 events (1 MiB) across feeds instead of 4,096 (scrolling.md).
5. Behaviour changes, all documented, none in the public API: style ids may be reused; refused
   or dropped clusters keep their base glyph instead of a blank; `grapheme_truncated` counts one
   per cut attach instead of one per extra mark (graphemes.md).

## Later (not this round; each needs a structural change)

| Fixture | After (proto) | Best engine | What is left |
|---|---:|---:|---|
| `scroll_region` | 278 | 756 | 43 region rows re-homed per LF (row-id scheme) |
| `scroll_region_small` | 27 | 79 | 24 tail rows re-homed per LF (row-id scheme) |
| `short_lines` | 55 | 129 | ~50 ns fixed bookkeeping + one event per LF |
| `cjk_wide` | 225 | 392 | per-glyph width lookup and UTF-8 decode |
| `tui_redraw` | 320 | 476 | CSI parse + CUP dispatch per 17-byte unit |
| `combining_unique` | 18 | 150 | one key allocation per unique cluster (needs a raw-entry table) |

## Open items

None. Root causes and gains were measured on this machine with a throwaway prototype of every
step (evidence.md); the prototype matched all 21 goldens, `csi_mix` included.
