# 0020: Kitty graphics protocol

**Goal:** `APC G ... ST` images. A program transmits RGB, RGBA or PNG data (raw or zlib, in one
sequence or in chunks), the engine decodes it to RGBA, keeps it under the program's image id, and
places it at the cursor through the pipeline Sixel already uses: an anchored `Placement`, cells
that name its `GraphicId`, pixels from `take_graphics`, `GraphicReleased` at the end. Queries are
answered, so `icat`, `chafa`, `timg`, `yazi` and `image.nvim` detect and use the protocol.

**Non-goals:** file, temporary-file and shared-memory transmission (`t=f`, `t=t`, `t=s`: the
engine does no file I/O for a stream, and a remote program names paths this machine does not
have); animation (`a=f`, `a=a`, `a=c`); Unicode placeholders (`U=1`); relative placements
(`P`, `Q`); cropping or scaling pixels in the engine; a bench fixture.

| File | Covers |
|---|---|
| [protocol.md](protocol.md) | routing, control keys, chunks, formats, decoding, replies, refusals, caps |
| [placement.md](placement.md) | image store, images and placements, footprint, cursor, new `Placement` fields, deletion |
| [tests.md](tests.md) | unit and terminal tests, docs, files, gate, commit |

**Feature:** `kitty-graphics`, in the default set, adds `png` and `miniz_oxide`. Without it
`APC G` stays what every APC is today: consumed and counted in `unhandled_sequences`. The
`--no-default-features` dependency set does not change.

**Public API:** `Placement` and `SnapshotPlacement` gain `image: GraphicId`,
`source_origin`, `source_size: (u32, u32)`, `pixel_offset: (u16, u16)`,
`display_size: (u32, u32)` and `z_index: i32`, with or without the feature (a Sixel placement
has `image == id`, the whole image, no offset, its `pixel_size` and layer 0).
`SnapshotPlacement` becomes `#[non_exhaustive]`. `GraphicReleased(id)` now ends a placement or
an image. All of it is breaking for a renderer: it draws `placement.image`, not `placement.id`.

## Acceptance criteria

- [ ] Quality gate; `cargo test -p oneterm-vt --no-default-features` (also with `vt-paranoid`);
      `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features`; public API snapshot updated
- [ ] `a=q`, `a=t`, `a=T`, `a=p`, `a=d` behave as protocol.md and placement.md say; every refusal
      row answers with its code
- [ ] `f=24`, `f=32`, `f=100`, each with and without `o=z`, in one sequence and in chunks, give
      the same pixels
- [ ] Hostile input never panics and stays inside the caps of protocol.md: declared sizes past
      4096, short data, a zlib or PNG bomb, bad base64, a chunk stream that never ends; placing
      a stored image costs no copy of its pixels
- [ ] A kitty placement scrolls, reflows and is released like a Sixel one
- [ ] `docs/user/graphics-kitty.md` exists and its examples run as doctests; limits,
      conformance and versioning chapters gain their rows
- [ ] Commit: `feat(graphics)!: kitty graphics protocol`

## Decisions

1. **Images and placements are two things**, as in the protocol. A stored image is one
   `Arc<GraphicData>` with a `GraphicId` of its own, handed to the embedder once, with its
   first placement. A placement has another `GraphicId` (the one its cells carry) and names the
   image, the source rectangle and the display size. The engine neither crops nor scales.
   Why not a copy per placement: `a=p` is ten bytes, and copying up to 64 MiB for each one
   lets a stream stall `feed` for tens of milliseconds per command.
2. **Cells own the placement**, as for Sixel: text printed over an image cell takes that
   cell, and the placement is released when its rows are gone. Kitty keeps images in a layer
   of their own; this engine does not, so a program that repaints text over an image loses
   those cells.
3. **The store is per screen.** An image id on the alternate screen does not replace the same
   id on the primary one. Both screens share one byte budget.
4. **`GraphicReleased` covers both ids.** A placement is released as before. An image is
   released once no placement shows it and the store no longer holds it; an image that was
   never handed out is forgotten silently.
5. **Direct transmission only.** Other media are refused with a reply, so clients fall back.
6. **An image without `i` and `I` is not stored**: nothing can name it again. Like a Sixel
   image it is its own placement, with one id.
7. **Kitty deletes touch kitty placements only**; a Sixel image is never deleted by `a=d`.
8. **Covering rule kept, by layer.** A newer placement whose extent contains an older one
   releases it only when the older one's `z_index` is not above the newer one's.

## Open items

None.
