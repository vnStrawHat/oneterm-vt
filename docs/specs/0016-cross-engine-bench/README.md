# 0016: Cross-engine benchmark (criteria, corpus, gate, harness)

**Goal:** one fair, repeatable comparison of oneterm-vt with eight embeddable VT engines on the
agreed machine (i7-12700, Windows 11): the same 23-fixture corpus, the same knobs, one
process per run, and no number without a verified end state. Approving this spec is the
agreement on criteria and machine that precedes any optimisation work.

**Scope:** the grid layer (parse + apply) and memory. Parser-only and resize are "Later" (criteria.md).

**Non-goals:** end-to-end/PTY/render timing; changing the engine; CI runs; a Linux or macOS
run (the scripts allow it later); kitty graphics (oneterm-vt lacks it); quoting results in
README or `docs/user/`.

| File | Covers |
|---|---|
| [criteria.md](criteria.md) | layer, metrics, markers, summary, statistics, knobs, machine, power plan |
| [corpus.md](corpus.md) | 23 fixtures, alignment with vtebench/kitty/Ghostty, new generators, vendored data, lock |
| [verification.md](verification.md) | checkpoints, canonical dump, goldens, statuses, accepted divergences |
| [harness.md](harness.md) | `compare/` layout, adapter trait and CLI, driver commands, pinning |
| [engines.md](engines.md) | pins, adapter calls, capabilities, libghostty-vt build, probe results, exclusions |
| [output.md](output.md) | results JSON, markdown, chart, machine capture, paths |
| [tests.md](tests.md) | V1-V9, H1-H12, D1-D9, X1, manual acceptance |
| [files.md](files.md) | files, sub-milestones 16a-16d, gate, commit |

## Acceptance criteria

- [ ] vt-bench: `FIXTURES` unchanged (14), `COMPARE_FIXTURES` (9) outside every release tier; V1-V9; root gate green
- [ ] `compare/` excluded from the root workspace; its fmt, clippy `-D warnings`, tests, X1 pass
- [ ] `fetch-engines.sh` + `compare build` build all 9 engines here at the engines.md pins
- [ ] `compare corpus` matches `corpus.lock`; `compare verify` gives a status for every pair
- [ ] goldens committed for every fixture with plurality >= 3; the rest listed in `ungated.toml`
- [ ] `compare run --machine i7-12700-win11` writes `.json`, `.md`, `.svg`; wall time recorded (expected 50-80 min, not gated); first run committed
- [ ] power plan is High performance during the run and the previous plan afterwards, also after Ctrl-C
- [ ] repeat run: >= 90% of `ok` medians within spread (tests.md manual 6)
- [ ] `acceptance.md` (by the coder): build log summary, statuses, divergences, timings

## Decisions

1. Primary layer = parse + apply to terminal state, 64 KiB chunks, 160x45, 10 000 lines.
2. Unsupported capability = `n/a`, never a number; diverged = number with `≠`, never charted.
3. Every engine is pinned (crate version, git commit, npm version, Zig hash) and built here.
4. oneterm-vt is one voter in the goldens, not the arbiter.
5. No 3-engine consensus on a fixture: no golden, every engine `unverified` there (shown with
   `?`, out of geomean and charts); nobody is judged right or wrong.
6. Runs are committed under `perf/compare/<machine>/<date>.{json,md,svg}` with the methodology;
   not quoted in the root README or user docs.
7. The 9 new fixtures are compare-only: generated in vt-bench (`COMPARE_FIXTURES`), never in
   release tiers, release bench, `grid --check` or history charts.
8. The driver sets High performance for a run and restores the previous plan on every exit
   path (criteria.md); both plans are recorded.

## Design choices with alternatives (A = as proposed, B = the alternative)

| # | A | B | Evidence |
|---|---|---|---|
| D1 | compare-only generators in vt-bench (separate table) | generators in `compare/` | `vt-bench grid --fixture X` + hotpath profile the exact cross-engine stream while optimising; B duplicates the generator helpers; release tiers unaffected either way |
| D2 | goldens by plurality >= 3 | oneterm-vt's state as the golden | B fails every engine for our bugs; weak probe evidence (naive text dumps): 5-7 of 9 engines agree on core fixtures |
| D3 | `compare/` outside workspace and CI | workspace member + CI job | heavy deps (wezterm image stack, Zig, Node) stay away from the crates; hosted runners too noisy |
| D4 | xterm.js fed by internal `writeSync` | public `write(data, cb)` | probe `plain_ascii` 101 vs 70 MiB/s: the gap is event-loop yields between 12 ms slices, idle time not engine work; pinned 6.0.0 keeps the internal stable |
| D5 | one process per (engine, fixture, repetition) | one process per engine | B carries heap, JIT and allocator state across fixtures |
| D6 | one P-core thread, high priority | unpinned | migration doubles spread on this hybrid CPU (docs/user/performance.md) |
| D7 | baseline CPU for all | native | shipped builds are baseline; Ghostty native vs baseline was within noise, but measured with its 10 000-byte scrollback cap, so 16d re-checks it |
| D8 | 4 checkpoints per verify | final screen only | periodic fixtures (`plain_ascii`, `scrolling`) end on the same screen whatever was dropped |
| D9 | libvterm scrollback ring (one `memcpy` per pushed line, preallocated), timed, labelled `+ring` | no storage | every other engine stores 10 000 lines |
| D10 | Ghostty: byte cap removed, 10 000 lines | Ghostty defaults | the default 10 000-byte cap keeps almost no history; probe `plain_ascii` 740 -> 130 MiB/s once fixed |

Rows are independent: B on a row reverts only that row's rules and tests.

## Later

Parser-only and resize layers: a follow-up spec (criteria.md "Later").

## Open items

None. The owner's four answers (2026-10-29) are decisions 5-8; the advisor's review is folded
in (verification.md, engines.md, criteria.md).
