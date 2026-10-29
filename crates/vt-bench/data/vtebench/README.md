# Vendored vtebench streams

Source: <https://github.com/alacritty/vtebench>, commit `ead80032e57dee2e75f0b51f2ea67528647d9944`.

| File here | Original path | Used by fixture |
|---|---|---|
| `symbols` | `benchmarks/unicode/symbols` | `unicode_symbols` |
| `vim_session` | `benchmarks/medium_cells/vim_session` | `vim_session` |

Both files are unchanged byte for byte (CR LF line ends with 2 bare LFs in `symbols` and 77 in `vim_session`; `.gitattributes` marks them `-text`).
vtebench is dual-licensed under Apache-2.0 and MIT: see `LICENSE-APACHE` and `LICENSE-MIT`.
