# Tests, files, gate

## Unit tests (`graphics/kitty_tests.rs`, `graphics/image_store_tests.rs`)

- Control data: every key of protocol.md; defaults; unknown key skipped; malformed pair, a
  value past `u32`, control data past 512 bytes all invalid; the source rectangle.
- Base64: padded, unpadded, split at every chunk boundary; a foreign byte marks the load.
- Decode: `f=24` and `f=32` exact, long and short data; `o=z` round trip; a zlib stream that
  inflates past its limit; PNG in RGB, RGBA, grey, grey + alpha, palette with `tRNS`, 16-bit;
  a truncated PNG; sides 0 and 4097.
- Store: replace by key, same id on both screens, newest by number, generated ids skip ids in
  use, byte and count eviction oldest first.

## Terminal tests (`terminal/kitty_tests.rs`)

- `a=q`: `OK`; each refusal row; `q=1` and `q=2`; no id, no reply.
- `a=T`: placement, cells, `take_graphics` pixels, reply with `i` and `p`; chunked equals
  unchunked; a continuation ignores its other keys; an aborted chunk drops the load.
- `a=t` then `a=p` twice: two placements, one image handed out once, `live_bytes` counted
  once; a repeated `p` replaces the first.
- Image releases: with the last placement of an unnamed image, with the store for a named one,
  never for an image that was not handed out; a pending image dropped at the queue ceiling
  is handed out again by its next placement.
- Transmitting an id again releases its placements.
- Source rectangle: `source_origin` and `source_size`, the whole image handed out; `w`/`h`
  clipped; outside the image refused.
- Footprint and `display_size` for the four `c`/`r` rows; `X`/`Y` clamped; columns clipped;
  rows bounded.
- Cursor: `C=0` at the left, at the right edge, at the bottom (scrolls, the anchor follows);
  `C=1` keeps it.
- Delete: every row of the table, both cases; a Sixel placement survives `d=A`; a placement in
  history survives `d=a`.
- Store per screen: the alternate screen's id 1 leaves the primary's alone.
- RIS forgets stored images. `release_covered` respects layers.
- Snapshot: `SnapshotPlacement` carries the new fields.
- Without the feature (`#[cfg(not(feature = "kitty-graphics"))]`): `APC G` is counted and
  nothing is placed.

## Files

| Path | Change |
|---|---|
| `Cargo.toml`, `crates/vt/Cargo.toml` | `png`, `miniz_oxide`; feature `kitty-graphics` in `default` |
| `graphics/kitty.rs` (new) | `Command`, the APC reader, base64, `Load`, decoding, `Refusal` |
| `graphics/image_store.rs` (new) | `ImageStore`, `StoredImage` |
| `graphics.rs` | the new `Placement` fields; `GraphicsState.kitty`; `next_graphic_id` |
| `graphics/placement.rs` | shared-image accounting in `admit`, `release`, `push_pending`; `retire`; layer-aware `release_covered` |
| `terminal/graphics.rs` | `place` takes a `Picture` and a `Layout` |
| `terminal/kitty.rs` (new) | the actions, replies, cursor rule, deletion |
| `terminal/dispatch.rs` | `apc_start` / `apc_put` / `apc_end` routing |
| `snapshot/state.rs` | the mirrored fields |
| `docs/user/` | `graphics-kitty.md` (new), `graphics.md`, `limits.md`, `conformance.md`, `versioning.md`, the README index |
| `.github/workflows/ci.yml` | clippy and tests for `--no-default-features --features kitty-graphics` |

## Gate

The quality gate of CLAUDE.md, plus `cargo test -p oneterm-vt --no-default-features` (alone,
with `vt-paranoid`, with `kitty-graphics`), the doc build, `bash scripts/public-api.sh update`,
and the `compare/` workspace gate with its lock file refreshed.
