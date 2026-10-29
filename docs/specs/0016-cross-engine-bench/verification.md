# Honesty gate: every number comes with a verified end state

A figure is published only if the engine consumed every byte and reached the expected state.
Three checks, all outside the timer:

1. **Verify pass** (own process): feed the fixture once, dump the canonical state at 4
   checkpoints: after chunk `ceil(n x q / 4)` for `q = 1..4`, `n` = chunk count, calling the
   engine's `finish()` before **every** dump (vte 0.15, used by alacritty and vt100, holds
   bytes inside a `?2026` window until it closes or is flushed). Catches engines that drop or
   defer early input, not only ones that get the last screen wrong.
2. **Golden compare**: the 4 dumps are compared with `compare/golden/<fixture>.json`.
3. **Timed-run tie**: every timed process prints the sha256 of the canonical final dump of
   its last pass (computed after the timer); it must equal the verify pass's final dump hash.
   So the configuration that was timed is the one that was verified.

## Canonical dump (built by the harness, not by each adapter)

```json
{ "cursor": [row, col],
  "rows": ["...45 strings..."],
  "history": ["...up to 45 strings, oldest first..."],
  "attrs": [[row, col_start, col_end, "fg", "bg", "flags"]],
  "links": [[row, col_start, col_end, "uri"]] }
```

| Field | Rule |
|---|---|
| `rows` | active screen, top to bottom; per column: wide-char spacer contributes nothing, empty cell contributes one space, else the cell's full text: the cluster, or for engines without clusters the base char followed by the combining marks they store beside it (alacritty zero-width chars, vt100 cell contents, libvterm `chars[]`); trailing spaces trimmed |
| `cursor` | active screen, 0-based; `col` clamped to `cols - 1` (pending-wrap representations differ) |
| `history` | the primary screen's scrollback rows directly above its top row, same text rule; at the final checkpoint only (earlier ones: empty) |
| `attrs` | maximal column runs (end exclusive) whose style is not all-default; a spacer column takes its lead cell's style |
| colours | `"d"` default, `"i<n>"` palette 0-255 (SGR 30-37, 90-97 map to 0-15), `"r<rrggbb>"` direct |
| `flags` | sorted subset of `b` bold, `i` italic, `u` any underline, `r` inverse; others ignored |
| `links` | only for `hyperlinks`; runs of equal URI |
| images | not dumped; for `sixel*` the cursor and text after each image carry the check |

Adapters only map an engine cell to `(text, width, fg, bg, flags, uri)`; trimming, runs,
clamping and hashing live in the harness (one implementation, unit-tested), plus a JS copy in
the xterm adapter tested against the same fixture dumps (tests.md).

## Goldens

- `compare golden` builds `compare/golden/<fixture>.json` from verify results: the dump set
  (all 4 checkpoints identical) shared by the most engines, if at least **3** engines share
  it. The file records `corpus_version`, the fixture sha256 and `agreeing_engines`.
- No set reaches 3: **no golden** is written; the command lists the groups and records the
  fixture in `compare/golden/ungated.toml` (fixture, corpus sha256, group sizes). Every
  engine's grid figure on that fixture is `unverified`: shown, never judged right or wrong,
  excluded from the geomean and the charts. Nobody picks a winner by hand.
- oneterm-vt is one voter like the others; when it disagrees with the golden it is flagged
  like any engine (that is a bug report for us, not a tie-breaker).
- Goldens are committed and reviewed as text; `golden` never overwrites an existing file
  without `--replace`.

## Statuses (per engine, fixture)

| Status | Condition | Number shown | In chart and geomean |
|---|---|---|---|
| `ok` | 4 dumps equal the golden; timed hashes equal the verify hash | yes | yes |
| `accepted` | differs, but `(engine, fixture, final sha256)` is in `compare/accepted.toml` | yes, marked `†` | yes |
| `diverged` | differs, not accepted | yes, marked `≠` | no |
| `unsupported` | capability not declared | `n/a` | no |
| `failed` | crash, timeout, bad JSON, or timed hash != verify hash | `fail` | no |
| `unverified` | memory figures (no end state); grid on an ungated fixture (no 3-engine consensus) | yes, marked `?` | no |

`compare/accepted.toml` (owner-written; agents never add entries):

```toml
[[accepted]]
engine = "vt100"
fixture = "cjk_wide"
final_sha256 = "..."
reason = "one-line semantic difference, e.g. Unicode width table version"
```

The hash pins the exact state: if the engine changes behaviour, the entry stops matching and
the pair is `diverged` again. Every `diverged` row gets the first difference in the report
appendix: checkpoint, field, row, column, expected vs got (80 chars each).

## What the probe showed (weak evidence: naive dumps, text only)

A second probe pass dumped each engine's screen as plain text (cursor line dropped, trailing
blank rows trimmed; Ghostty through its plain-text formatter) and grouped identical hashes:
`scroll_region` 7 of 9 engines agree, `plain_ascii`, `tui_redraw`, `mode_churn`,
`long_lines` 5-6 of 9, `cjk_wide` 5 of 9, `grapheme_clusters` 0 (9 different screens). The
reasons for the minority groups (spacer cells printed, scrollback included, a trimmed last
row) come from reading a few diffs by eye and were not fixed or re-run. In the first sweep's
own output the Ghostty hash column is constant: its dump was not wired yet, so it says
nothing. Ghostty also ran with its default 10 000-byte scrollback cap in both passes. So plurality goldens work for core
fixtures; `grapheme_clusters` (and possibly `unicode_symbols`) are expected to be ungated.
An ungated fixture still gets the verify pass and the timed-run tie (check 3), so a crash or
a non-deterministic end state is still `failed`; capability rules still apply.
