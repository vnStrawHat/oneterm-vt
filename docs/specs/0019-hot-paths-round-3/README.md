# 0019: Engine hot paths, third round

**Goal:** close measured gaps of `perf/compare/i7-12700-win11/2026-10-30.md` in `crates/vt`
(OSC/DCS payloads, OSC text, Sixel, line feeds, region rotation) in steps landed one after
another, gate green after each; every step proven in a prototype (evidence.md).

**Non-goals:** new public items or `Dispatch` methods; event coalescing; dropping OSC payload
limits.md promises; corpus, goldens, harness. What stays behind: end of evidence.md.

| File | Covers |
|---|---|
| [evidence.md](evidence.md) | method and tools, profiles, per-step prototype numbers, re-checks, rejected experiments, verify, what stays behind |
| [parser.md](parser.md) | S1 OSC and DCS payload runs |
| [osc-text.md](osc-text.md) | S2 strict-first OSC decode, borrowed link key, `text_span` without re-validation |
| [sixel.md](sixel.md) | S3 decode buffer handed over as the image; S4 id sweeps over the `occ` prefix |
| [scrolling.md](scrolling.md) | S5 line feed into history without the region scroll; S6 boxed ring rows |
| [tests.md](tests.md) | changed and new tests per step, chunk invariance, compare verify |
| [files.md](files.md) | step order, files per step, thresholds, gate, user docs, commits |
| `acceptance.md` | written by the coder: before/after per step, verify output, compare run |

## Acceptance criteria

- [ ] Steps S1-S5 landed in order (files.md), gate green after each, one commit each
- [x] S6 landed last only if the owner approves it (below); otherwise its section is skipped
- [ ] Every per-step threshold in files.md met (pinned `vt-bench grid`, median of 5, 32 MiB)
- [ ] No guard fixture below 0.95 x its value before the step; `vt-bench grid --check` passes
- [ ] `compare verify --engine oneterm` 21/21 ok; public API snapshot unchanged (CI)
- [ ] `compare run --machine i7-12700-win11` re-run and committed; `acceptance.md` filled

## Steps and expected gains (prototype, pinned MiB/s; best engine from the compare run)

| Step | Root cause (measured) | Fix | Target: before -> after | Best engine |
|---|---|---|---|---:|
| S1 | every OSC and DCS payload byte goes through the per-byte table (94 % of `long_osc`) | payload runs: one copy per OSC run, a tight `dcs_put` loop (ghostty's and rio's technique) | `long_osc` 904 -> 9,931, `hyperlinks` 278 -> 867, `osc_9_7` 238 -> 286 | 13,320, 974, 495 |
| S2 | lossy decode with no ASCII path, two boxed keys per link lookup, `text_span` validates twice | strict-first decode, borrowed link key, span straight from `append` | `hyperlinks` 867 -> 1,220, `osc_9_7` 286 -> 344 | 974, 495 |
| S3 | Sixel decode buffer grown by doubling, then copied pixel by pixel into a new image | raster-sized buffer handed over as the image (`into_flattened`) | `sixel_large` 25.7 -> 42.0, `sixel` 46 -> 54 | 34, 79 |
| S4 | the extras sweep reads 160 cells x 10,045 rows every 4,096 images (15 % of `sixel`) | read the `occ` prefix and the last cell | `sixel` 54 -> 65 | 79 |
| S5 | each LF runs the general region scroll and walks the anchor list | whole-screen LF fast path; anchor walks only when an anchor can be below `oldest` | `short_lines` 58.6 -> 65.8, `osc_9_7` 329 -> 361, `sixel` 65 -> 72 | 128, 495, 79 |
| S6 (owner) | a region rotation moves 48-byte slots (`memmove` 14-17 %) | boxed ring rows: 8-byte slots (ghostty rotates row handles) | `scroll_region_small` 59.8 -> 64.3, `scroll_region` 558 -> 616 | 79, 753 |

Targets: interleaved best of 3 where the single run was noisy (evidence.md). HEAD -> S5
elsewhere: `plain_ascii` 745 -> 815; every other fixture within noise (re-checks).

## Decisions

1. String payloads are taken in runs inside the parser; `Dispatch` does not change (S1). A
   slice-taking `dcs_put` (rio's `put_slice`) is a new public trait method for a call that is
   about 13 % of `sixel` after S1-S6: not prototyped, not proposed.
2. Unrouted OSCs keep their 2 KiB and their `truncated_osc` count (limits.md). Ghostty's
   13 GB/s on `long_osc` comes from storing nothing for an unknown OSC number.
3. S5's `Anchors::trim` returns early when only screen-owned anchors could be below `oldest`:
   none ever are, and every caller writes them back (scrolling.md).
4. No `unsafe`, no new dependency, no public API change, no change to events, counters or
   limits in any step. Measured and not proposed (pen style memo, one UTF-8 check per ground
   run, 16-byte `ESC` look-ahead): evidence.md, "Rejected experiments".

## Owner decision (approved 2026-11-12): S6, boxed ring rows

Not an API or behaviour change, a trade-off. For: `scroll_region` +10-14 %,
`scroll_region_small` +7 % (interleaved, plain and aligned builds), and the ring shrinks 6x
(768 KiB -> 128 KiB at 10,000 lines, 48 -> 8 MiB at the 1,000,000 maximum; `grid.md`
changes). Against: `scrolling` -4 % aligned / -6 % plain (a pointer per row access on its
path, past the 5 % guard), `short_lines` -1 to -3 %, one 48-byte box per written row.
Without S6 the other steps are unaffected; the spec lands it last.
