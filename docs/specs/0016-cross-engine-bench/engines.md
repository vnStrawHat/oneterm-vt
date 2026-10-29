# Engines: pins, build, adapter calls, feasibility on this machine

Probe (2026-10-29, `.tmp/compare-probe/`, Windows 11, rustc 1.96.0, MSVC Build Tools 2022
17.14): every engine below **built and ran** all 14 current fixtures at 16 MiB, 160x45,
pinned with `start /affinity 10 /high`. No engine needs a Linux fallback.

**Probe numbers are not results.** In the first sweep Ghostty ran with its default
scrollback cap of 10 000 **bytes** (`max_scrollback_bytes`, `src/terminal/Terminal.zig`), i.e.
almost no history, while the others kept 10 000 lines. Re-run with the cap removed and 10 000
lines: `plain_ascii` 740 -> 130, `scrolling` 1107 -> 276, `long_lines` 1096 -> 255 MiB/s;
`heavy_sgr` (270), `tui_redraw` (392), `dense_cells` (225) unchanged. Only verified runs under
this spec count.

## Pins

| Engine | Pin | Language, build | Source location |
|---|---|---|---|
| `oneterm` | this repo | Rust, path `crates/vt`, `default-features = false` | repo |
| `ghostty` (libghostty-vt) | ghostty-org/ghostty `b2c78c9bd9bc88a16d9c3769b0fb17ddde1b78b1` (1.4.1-dev, 2026-10-28) | Zig 0.17.0 (`zig-x86_64-windows-0.17.0.zip`, sha256 `b5663f69581dcf391293fbf16c06cb80d81d806545ce618b4d0bab7f0eb8c428`), `ReleaseFast`, static `ghostty-vt-static.lib` | `.cargo-home/compare/src/ghostty` |
| `alacritty` | crate `alacritty_terminal = "=0.26.0"` (vte 0.15.0) | Rust | crates.io |
| `rio` | crate `rio-vt = "=0.5.28"`, `default-features = false` (no PTY) | Rust | crates.io |
| `wezterm` | wezterm/wezterm `e68a8ec27727fbbda15e07f0a0737cd5e16481f6` (2026-10-26), package `wezterm-term` 0.1.0 (not on crates.io) | Rust, path dep | `.cargo-home/compare/src/wezterm` (no submodules) |
| `vt100` | crate `vt100 = "=0.18.1"` (vte 0.15.0) | Rust | crates.io |
| `avt` | crate `avt = "=0.19.0"` | Rust | crates.io |
| `libvterm` | neovim/libvterm mirror `934bc2fbf21800ac3458a499df8820ca5fb45fd3` (0.3.x) | C via `cc` (MSVC `/O2`), static | `.cargo-home/compare/src/libvterm` |
| `xterm` | npm `@xterm/headless` `6.0.0`, Node `v22.20.0` | TypeScript/JS (V8) | `compare/engines/xterm/node_modules` |

Each Rust adapter commits its `Cargo.lock`; `fetch-engines.sh` holds the three commits and
the Zig hash as variables at its top. Bumping a pin is a one-line change plus new
verify/golden runs.

## Adapter calls

| Engine | create / feed / finish | dump source |
|---|---|---|
| `oneterm` | `Terminal::new(Size, Config { scrollback_limit: 10_000, .. })`; `feed(chunk, &mut batch, now)` then `take_graphics()` per chunk, on every fixture (as vt-bench tier 2, so the numbers match it; an embedder must drain images, and on a chunk without images it is an empty-`Vec` swap) | public grid/cell API |
| `ghostty` | `ghostty_terminal_new`; before the first write `ghostty_terminal_set(SCROLLBACK_MAX_BYTES, NULL)` (removes the 10 000-byte default cap) and `ghostty_terminal_set(SCROLLBACK_MAX_LINES, &10000)`, both checked for success; `ghostty_terminal_vt_write`. The line limit is page-granular: whole pages are pruned, so it holds about 9 785 rows after `scrolling` (measured), noted in the report | `ghostty_terminal_grid_ref`, `ghostty_grid_ref_cell/style/graphemes/hyperlink_uri`, `ghostty_terminal_get(CURSOR_X/Y)`; verify also asserts `SCROLLBACK_ROWS >= 9 000` after `scrolling` |
| `alacritty` | `Term::new(Config { scrolling_history: 10_000 }, &TermSize, VoidListener)`; `vte::ansi::Processor::advance`; finish: `stop_sync` if bytes are pending | `term.grid()[Line][Column]`; zero-width chars appended to the cell text |
| `rio` | `Crosswords::new(size, CursorShape::Block, VoidListener, WindowId::from(0), 0, 10_000)`; `performer::handler::Processor::advance` | `visible_rows()` / grid squares |
| `wezterm` | `Terminal::new(TerminalSize, Arc<config>, .., Box::new(io::sink()))` with `scrollback_size() = 10_000`; `advance_bytes` | `screen().lines_in_phys_range`, cell attributes |
| `vt100` | `Parser::new(45, 160, 10_000)`; `process`; `finish`: no public flush, so the corpus closes every `?2026` window it opens (all 23 fixtures do; V-test in tests.md) | `screen().cell(row, col)` (contents include combining marks) |
| `avt` (label `+utf8 decode`) | `Terminal::with_scrollback(160, 45, Some(10_000))`; `feed(&str)`, zero-copy: `str::from_utf8` on the chunk slice itself, feed the valid prefix, keep at most 3 tail bytes as carry (joined with the next chunk's head in a 4-byte stack buffer), feed U+FFFD as its own `char` for each invalid sequence. Inside the timer (avt takes `&str` only; every other engine decodes in its own feed) | `screen()` lines and cells |
| `libvterm` (label `+ring`) | `vterm_new`, `vterm_set_utf8(1)`, `vterm_screen_reset(1)`; `sb_pushline` does one `memcpy` of the `VTermScreenCell` row into a ring preallocated once at create (10 000 x 160 cells): no per-cell work, no per-line allocation (timed: libvterm stores no scrollback, every embedder must); `vterm_input_write` | `vterm_screen_get_cell` (`chars[]` joined), ring |
| `xterm` | `new Terminal({cols, rows, scrollback: 10000, allowProposedApi: true})`; `term._core._writeBuffer.writeSync(chunk)` (internal; D4). Any addon used (`@xterm/addon-unicode-graphemes`) is pinned to an exact version in `package.json` and the lock | `buffer.active.getLine().getCell()` |

## Capabilities (declared by the adapter; the gate checks the claim)

| Engine | `sixel` | `graphemes` | `hyperlinks` |
|---|---|---|---|
| oneterm | yes | yes (`?2027`) | yes |
| ghostty | no (not implemented upstream) | yes | yes |
| alacritty | no | no | yes |
| rio | yes | yes (on by default) | yes |
| wezterm | yes | yes (always clusters) | yes |
| vt100 | no | no | no |
| avt | no | no | no |
| libvterm | no | no | no |
| xterm | no (`@xterm/addon-image` targets the browser renderer; coder checks headless) | if the pinned `@xterm/addon-unicode-graphemes` loads headless | if `_core._oscLinkService` is readable |

xterm.js runs V8 JIT and GC helper threads on the same pinned core as its main thread; the
report says so next to its column.

## Building libghostty-vt here (what the probe needed)

- `zig build -Demit-lib-vt=true -Doptimize=ReleaseFast -Dtarget=x86_64-windows-msvc
  -Dcpu=baseline --prefix .cargo-home/compare/libghostty-vt`, with
  `ZIG_GLOBAL_CACHE_DIR=.cargo-home/zig` and `ZIG_LOCAL_CACHE_DIR=.tmp/compare/zig-local`
  (Zig defaults to `%LOCALAPPDATA%`, outside the project). About 70 s; outputs
  `lib/ghostty-vt-static.lib` (12 MiB), `include/ghostty/vt.h`.
- **Proxy**: this machine's HTTPS proxy answers Zig's own fetcher with `400 Bad Request`
  (curl and git work). `fetch-engines.sh` therefore runs `zig build`, collects every failing
  `.url`, downloads it with `curl` (tarball) or `git -c core.autocrlf=false clone` + checkout
  (`git+https` URLs), runs `zig fetch <dir>`, and repeats until the build stops failing
  (9 packages at this pin). `core.autocrlf=false` is required: CRLF checkouts change the
  package hash.
- Zig 0.17 unpacks packages into `<ghostty>/zig-pkg/`; leave it there.
- Linking: `build.rs` emits `rustc-link-search` to `$LIBGHOSTTY_VT_DIR/lib` (default
  `.cargo-home/compare/libghostty-vt/lib`) and `rustc-link-lib=static=ghostty-vt-static`; no
  extra system libraries were needed. FFI declarations are hand-written (about 15 functions,
  `repr(C)` option structs with their `size` field set); no bindgen.
- Native vs baseline CPU at this pin: within noise (`plain_ascii` 828 vs 751, `tui_redraw` 406
  vs 409 MiB/s), but measured with the 10 000-byte scrollback cap (above), so weak evidence;
  the library dispatches SIMD at run time. Baseline is used (criteria.md); 16d repeats the
  native/baseline check with 10 000 lines and reports it in `acceptance.md`.

## Not included, and why

| Engine | Reason |
|---|---|
| kitty | no embeddable core: the VT state lives in a CPython extension of the app; its benchmark kinds are in the corpus instead |
| Windows Terminal / conhost core | C++ inside the app, no standalone library or headless build |
| GNOME VTE (C) | a GTK widget; no headless use on Windows |
| termwiz (alone) | parser + surface, no terminal state; its state machine is measured through `wezterm-term` |
| libtsm | C, small and little maintained; libvterm already covers the C-library class |
| vt100-ctt | a fork of `vt100` |
| foot, st, xterm, iTerm2 | applications without a library API, or not on Windows |
