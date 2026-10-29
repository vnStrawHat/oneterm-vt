# Corpus v1: 23 fixtures

One generator source: `crates/vt-bench/src/fixtures.rs`. The 14 existing fixtures
(`FIXTURES`) stay byte-identical and remain the only ones in the release tiers, release
bench, `grid --check` and the history charts. The 9 new ones are **compare-only**, in a
separate table `COMPARE_FIXTURES`: `vt-bench fixtures --out DIR --mib 32 --compare` writes all
23, and `--fixture NAME` on vt-bench's `parser`/`grid`/`render` also finds them, so one stream can be
profiled with hotpath during optimisation (`--fixture` never writes a record). The compare
driver hashes the 23 files against `compare/corpus.lock`.

`core` fixtures run on every engine and are gated. `capability` fixtures run only on engines
whose adapter declares the capability; the others get `unsupported`, never a number (an engine
that skips a DCS/OSC it does not implement is fast for the wrong reason; the probe showed
alacritty, vt100 and libvterm "reaching" 600-1000 MiB/s on `sixel_large`).

## Fixtures

| # | Name | Class | Content | Aligned with |
|---|---|---|---|---|
| 1 | `plain_ascii` | core | ASCII log lines, CR LF | Ghostty `ascii` with lines; kitty "Only ASCII" |
| 2 | `long_lines` | core | lines 5x the width, autowrap | Ghostty `ascii` unbroken |
| 3 | `heavy_sgr` | core | 24-bit SGR per char | Ghostty SGR-heavy `styled` |
| 4 | `tui_redraw` | core | CUP + short text | Ghostty TUI redraw |
| 5 | `scroll_region` | core | DECSTBM 2;44, line feeds | vtebench `scrolling_bottom_region` |
| 6 | `cjk_wide` | core | wide CJK fills each line | Ghostty CJK, kitty "Unicode" |
| 7 | `dense_cells` | core | 256-colour SGR per cell, full screen | vtebench `dense_cells` |
| 8 | `scrolling` | core | grid-width lines | vtebench `scrolling_fullscreen` |
| 9 | `sixel` | capability `sixel` | small Sixel images | kitty "Images" (protocol differs) |
| 10 | `osc_9_7` | core | OSC 9;7 before every line | Ghostty `osc` |
| 11 | `grapheme_clusters` | capability `graphemes` | ZWJ, VS16, flags, marks under `?2027` | Ghostty `?2027` variants |
| 12 | `hyperlinks` | capability `hyperlinks` | OSC 8, 200-1000 byte URIs | Ghostty `styled --osc8-rate` |
| 13 | `mode_churn` | core | alt screen, modes, `?2026`, titles | real TUI open/close |
| 14 | `sixel_large` | capability `sixel` | 640x480-1024x768 images | kitty "Images" size class |
| 15 | `cursor_motion` | core | spiral of CUP + one char per ring cell, one letter per unit | vtebench `cursor_motion` |
| 16 | `light_cells` | core | prefix `?1049h`; unit `CSI H` + 160x45 copies of one letter | vtebench `light_cells` |
| 17 | `short_lines` | core | unit `y` CR LF | vtebench `scrolling` |
| 18 | `scroll_region_small` | core | prefix `?1049h` `CSI 1;22 r`; unit `y` CR LF | vtebench `scrolling_*_small_region` |
| 19 | `csi_mix` | core | weighted pick per unit (below) | kitty "CSI codes with few chars" |
| 20 | `combining_unique` | core | `a` + 3 marks from U+0300..U+036F, the triple unique per unit, autowrap | kitty "Unique multi-codepoint cells" |
| 21 | `long_osc` | core | `OSC 6 ;` + 8 000 printable bytes + BEL, then `osc N` CR LF | kitty "Long escape codes" |
| 22 | `unicode_symbols` | core | vendored vtebench `symbols` (135 KiB, one wrapping line) | vtebench `unicode` |
| 23 | `vim_session` | core | vendored vtebench `vim_session` (174 KiB zsh + vim recording) | vtebench `medium_cells`; the real-world stream |

Kitty "Images" (kitty graphics protocol) is **not** in v1: oneterm-vt does not implement it, so
it has no row to compare against; add it with the protocol.

## New generators (same `build`/`put`/`pick` helpers, no RNG state)

- `cursor_motion`: per unit `i`, letter `A + i % 26`; rings `k = 0..` while the ring is
  non-empty: top edge left to right, right edge down, bottom right to left, left edge up, each
  cell `CSI row;col H` + letter. One unit is one full spiral (about 7 200 CUPs at 160x45).
- `csi_mix`: per unit `pick(i, 100)` selects: `< 10` printable run of 1-72 bytes, CR LF after;
  `< 30` `CSI m` `CSI ?1h` `CSI H`; `< 40` SGR bold+faint+italic+curly underline (`4:3`) + fg 31;
  `< 50` colon-form `38:5:n` and `48:2::r:g:b`; `< 60` underline colour `58;5;n` + faint;
  `< 80` `CSI m` `CSI 10A` `CSI 3E` `CSI 2K`; else `CSI 39m` `CSI 10 \`` `a` `CSI 100 b` `CSI ?1l`.
  Our own sequences of kitty's kinds, not kitty's bytes.
- `combining_unique`: unit `i` = `a` + U+0300 + `(i / 112^k) % 112` for `k = 0..3`.
- `long_osc`: payload bytes `letter(pick(i << 16 | n, 26))`, `n = 0..8000`.
- `unicode_symbols`, `vim_session`: `include_bytes!` of the vendored file, unit = whole file.
  The files mix CR LF with bare LF (`symbols`: 2 bare LFs; `vim_session`: 77 bare LFs;
  counts from the git blobs), and they are fed unchanged: no ONLCR translation is
  applied, so every engine sees the same bytes.

## Vendored data

`crates/vt-bench/data/vtebench/`: `symbols`, `vim_session`, `LICENSE-APACHE`, `LICENSE-MIT`,
`README.md` (source `github.com/alacritty/vtebench` commit
`ead80032e57dee2e75f0b51f2ea67528647d9944`, files unchanged, dual Apache-2.0/MIT).
About 310 KiB in git, once.

## Size, storage, versioning

| Item | Rule |
|---|---|
| size | 32 MiB per fixture (`--mib 32`), whole units; total about 740 MiB |
| storage | generated into `.tmp/compare/corpus/` (never committed); regenerated in about 10 s |
| lock | `compare/corpus.lock` (committed): `version = 1`, `mib = 32`, per fixture name, class, capability, bytes, sha256 |
| drift | `compare corpus` refuses to run on any hash mismatch; changing a generator means `--update-lock`, `version + 1`, and new goldens (verification.md) in the same commit |
| capability names | `sixel`, `graphemes`, `hyperlinks` |
