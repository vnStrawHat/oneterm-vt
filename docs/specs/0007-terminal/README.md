# 0007: Terminal, events and dispatch (phase 6)

**Goal:** root `Terminal` + `Config`: bytes in, grid mutated, an `EventBatch` of `VtEvent`s and
per-call `FeedStats` out. CSI/ESC/C0/OSC/DCS dispatch, modes, charsets, OSC routing, colour
overrides, title stack, byte-exact replies; bench tiers 2, 4, 5.

**Non-goals:** snapshot, `?2026` timeout, `mode_snapshot`, `mouse_reporting`, `set_theme_colors(&Palette)`
(7); search (8); `encode_key*` (9); Sixel, `take_graphics`, `placements`, `set_cell_pixels` (10);
`--check` (12); DECSLRM, DECSCA/DECSED/DECSEL. Where later phases hook in: terminal.md.

| File | Covers |
|---|---|
| [events.md](events.md) | `EventBatch` arena and spans, every `VtEvent`, `FeedStats` counters |
| [terminal.md](terminal.md) | `Config`, `Terminal`/`State` layout, methods, `feed` steps, hooks for phases 7-10 |
| [dispatch-esc-c0.md](dispatch-esc-c0.md) | handler, C0/C1, ESC, print path, charsets, `?2027`, REP, DCS/APC, unhandled rules |
| [dispatch-csi.md](dispatch-csi.md) | CSI table, cursor clamps, DECSTBM, SGR |
| [modes.md](modes.md) | `Mode`, `ModeState`, alt screens, cursor style, kitty flags, `modifyOtherKeys`, RIS, DECSTR |
| [osc.md](osc.md) | `OscRoute(s)`, `BUILTIN`, every builtin, OSC 52, title stack |
| [colors.md](colors.md) | `ColorKey`, `ColorOverrides`, colour specs, OSC 4/10-12/17/19/104/110-112 |
| [queries.md](queries.md) | byte-exact reply table, product name, DECRQCRA, DECRQSS, XTGETTCAP |
| [limits.md](limits.md) | every ceiling, hostile streams, counters |
| [bench.md](bench.md) | tiers 2/4/5, `CountingAllocator`, CLI, record |
| [tests.md](tests.md) | unit, integration, proptest, user chapters 03/08/09 |
| [files.md](files.md) | files, grid/intern/selection edits, `lib.rs`, rules, commit |

**Sub-milestones** (gate green after each): **6a** events, `Terminal` skeleton, grid report
edits, print + C0 + unhandled counting, resize, selection wrappers, `set_scrollback_limit`;
**6b** CSI/ESC, modes, charsets, SGR, alt screens, resets, `?2027`, REP, kitty, DECSCUSR,
grapheme sweep; **6c** OSC routes, builtins, colours, title stack; **6d** every reply + user
chapters; **6e** bench tiers.

## Acceptance criteria

- [ ] Quality gate; `cargo test -p oneterm-vt` with `--no-default-features` and `--features vt-paranoid`; `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features`
- [ ] A byte-exact test per reply row (queries.md) and a test per `FeedStats` field (tests.md)
- [ ] Every test in tests.md and every hostile row of limits.md passes; proptest included
- [ ] `docs/user/{events,limits,conformance}.md` exist; their Rust blocks run under `cargo test --doc`
- [ ] Root public items = the roadmap's phase-6 list; `unsafe` only in `vt-bench/src/memory.rs`
- [ ] `vt-bench grid|resize|rss|all` work; numbers in bench.md "Acceptance notes"
- [ ] Commit: `feat(terminal): Terminal, VT dispatch, events, OSC routes and replies` (minor)

## Decisions (* = chosen over a simpler alternative; advisor-reviewed)

1. One `StringTerm`: the root re-exports `parser::StringTerm`.
2. * Wraps, HT and every scroll path return their `ScrollReport`; no `RowsTrimmed` or `rows_scrolled` is lost to an implicit wrap.
3. * `FeedStats` has 12 fields, interner/grid counters wired as deltas (none reserved at 0).
4. * Relative moves clamp at the margins; only CUP/HVP/VPA are origin-relative (no CUU/CHA DECOM quirk); DECOM homes on set and reset.
5. * DECSTBM clamped to an empty region is ignored (grid invariant `top < bottom`).
6. * `?47`/`?1047`/`?1049` per spec 0004 (xterm); only 1049 saves/restores the cursor, so DECSC survives 47/1047.
7. * SGR `21` = double underline (ECMA-48, xterm, our `Attrs` doc), not cancel-bold.
8. DECSCA/DECSED/DECSEL stay out (counted); specs 0003/0004 had pencilled them in; the roadmap does not have them.
9. * End-of-feed grapheme sweep (spec 0003 promise).
10. CAN/SUB count as unhandled (spec 0002 D3).
11. * Behaviour extensions: empty OSC 0/2 -> `TitleReset`; `CursorStyleChanged` on every `cursor_style()` change (DECSCUSR, `?12`, `?25`, DECSTR, RIS), not only OSC 50.
12. Forwarded OSCs: the 16th param is re-split, one span per `;`-separated parameter (linear in the payload; large only with `large(n)`). * OSC 7 rejoins `;` and decodes only `file://`; * OSC 52 base64 canonical only.
13. OSC colour queries stay events; `ColorKey::color_reply` (wrapping `query_prefix`) pins the reply bytes.
14. DA1 claims Sixel from phase 6 (lands in 10, before v0.1.0); `CSI 14 t` answers the effective cell (`VIRTUAL_CELL` until `set_cell_pixels`, phase 10).
15. Public surface (terminal.md) deliberately has no `grid_mut`/`interner_mut` (`scroll_viewport` covers scrolling back and, with a large positive delta, scroll-to-bottom), no OSC 133 anchors, no `log`.
16. * Deliberate contract choices: `ColorKey` drops the never-written `BrightForeground`/`DimForeground`/`Dim(u8)` (selection keys keep indices 269/270; 259..=268 is a gap) and is `#[non_exhaustive]`; `FeedStats` and `ClipboardKind` are `#[non_exhaustive]` (minor-version notes, events.md).
17. `set_theme_colors(&mut self, &Palette)` moves to phase 7 with `Palette` (nothing reads it before).
18. `Config` stays exhaustive; `product_name: Option<Cow<'static, str>>`; a new field is a minor bump, stated in its rustdoc.

## Open items

None. Every candidate question was settled by the standards or an earlier spec of
ours; the advisor's verdicts are folded in.
