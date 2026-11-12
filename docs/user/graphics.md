# Graphics

A program draws an image with Sixel (`DCS q ... ST`) or with the kitty graphics protocol
(`APC G ... ST`, see [graphics-kitty](graphics-kitty.md)). The engine decodes it to RGBA, anchors a
placement to the cells it covers and tells you when it is gone. It never crops, rescales, draws or
uploads: pixels come from `Terminal::take_graphics`, positions from `SnapshotState::placements`,
the end of an image or a placement from `VtEvent::GraphicReleased`.

## Example: one pixel from bytes to texture and back

```rust
use std::collections::HashMap;
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, Size, SnapshotState, Terminal, VtEvent};

let now = Instant::now();
let mut term = Terminal::new(Size { rows: 4, cols: 10 }, Config::default());
term.set_cell_pixels(8, 16); // your font's cell, in device pixels
let (mut batch, mut state) = (EventBatch::new(), SnapshotState::new());
let mut textures = HashMap::new(); // your GPU textures, keyed by GraphicId

// DCS q, a 1x1 raster, register 0 = red, one sixel column, ST.
term.feed(b"\x1bPq\"1;1;1;1#0;2;100;0;0#0@\x1b\\", &mut batch, now);
for image in term.take_graphics() {
    assert_eq!(image.rgba.len(), 4); // 1x1 pixels, RGBA8, straight alpha
    assert_eq!(image.rgba, [255, 0, 0, 255]);
    textures.insert(image.id, image); // "upload"
}
for event in batch.iter() {
    if let VtEvent::GraphicReleased(id) = event {
        textures.remove(id); // free; an id you hold no texture for is a placement
    }
}
term.snapshot_update(&mut state, now);
let placement = state.placements()[0];
assert_eq!((placement.cols, placement.rows), (1, 1)); // one 8x16 cell
assert!(textures.contains_key(&placement.image)); // the pixels this placement shows
assert_eq!(state.rows()[0].cells[0].graphic, Some(placement.id)); // cells name the placement

term.feed(b"\x1bc", &mut batch, now); // RIS: the grid is empty again
assert!(batch.iter().any(|event| *event == VtEvent::GraphicReleased(placement.image)));
```

## Images and placements

```text
pixels  --decode--> GraphicData { id, rgba }  --take_graphics--> your texture, keyed by its id
        --place at the cursor--> Placement { id, image, .. } + cells that name the placement id
              --snapshot_update--> SnapshotPlacement { row, col, cols, rows, image, source, .. }
   rows leave the grid / are erased / evicted --> GraphicReleased(placement), GraphicReleased(image)
```

- A placement is an anchor on its top-left cell: it scrolls into history, follows IL / DL / SU /
  SD and survives reflow with its rows. It lives while its anchor lives and a row of its extent
  still carries an image cell. Text printed over an image cell replaces that cell only.
- `Placement::image` names the pixels. A Sixel image is its own placement (`image == id`); a
  kitty image has an id of its own and may be shown by many placements. An image is released
  after its last placement, or, if a program stored it, when the program deletes or replaces it.

## Embedder checklist

- Per cycle, in this order: `feed`, then `take_graphics` (upload), then the batch (`GraphicReleased`
  frees), then `snapshot_update`. `take_graphics` is the only drain; each image is handed out once,
  even one released before you took it (the release that follows frees it).
- Call `Terminal::set_cell_pixels(width, height)` on every font or DPI change.
- Key textures by `GraphicData::id`, draw `placement.image`. Ids start at 1 and are never reused.
- `GraphicReleased(id)`: forget what you keyed by `id`. It arrives in the next `feed`, an empty one
  included, and before `Repaint`. Which images are released, and in which `feed`, can depend on
  where the input was split: the pending ceiling and the sweep both run per `feed`.

## Drawing

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, Size, SnapshotState, Terminal};

let now = Instant::now();
let mut term = Terminal::new(Size { rows: 6, cols: 20 }, Config::default());
let (mut batch, mut state) = (EventBatch::new(), SnapshotState::new());
term.feed(b"\x1bPq\"1;1;25;45~-~-~-~-~-~-~-~\x1b\\", &mut batch, now); // 3x3 cells at 10x20
term.snapshot_update(&mut state, now);

let row = &state.rows()[1];
let id = row.cells[2].graphic.expect("the cell is covered");
let (dx, dy) = state.graphic_offset(id, row.id, 2).expect("the placement is live");
assert_eq!((dx, dy), (2, 1)); // draw the picture at cell_origin - (dx, dy) * cell_size
```

- The picture of a placement: `source_size` pixels of `image` at `source_origin`, stretched to
  `display_size`, at the top-left cell's origin plus `pixel_offset`. Sixel shows the whole image
  unscaled; kitty may cut and stretch. `z_index` below 0 goes under the text.
- By cells (recommended): for a cell with `graphic: Some(id)`, `graphic_offset(id, row.id, col)`
  gives `(dx, dy)` in cells; draw the picture at `cell_origin - (dx, dy) * cell_size`, clipped to
  that cell. Cells whose text replaced the image, and rows split by IL, then draw correctly.
  `None` means the placement was released: draw nothing.
- By placement (simpler, ignores overwrites): the picture at the placement's top-left cell, clipped
  to `cols x rows`. `placements()` lists both screens, oldest first. Draw text on top.

## Geometry (Sixel)

- Footprint: `ceil(pixels / cell)` per axis against `set_cell_pixels`; when either axis is 0 the
  `VIRTUAL_CELL` (10 x 20) is used for both. Columns are clipped to the screen, rows are not
  clipped at the bottom. The footprint is fixed when the image is placed.
- Cursor: it walks `bands above the last * 6 / cell_height` rows and keeps its column (the DEC
  rule). It ends on the image's last row, or below it when the payload ends in `-` and the cell
  height divides the image.
- Raster attributes (`"Pan;Pad;Ph;Pv`) set the image size; data past it is cropped and missing
  data is transparent. `CSI 14 t` answers with the effective cell, `VIRTUAL_CELL` until you set one.

## Ceilings

| Ceiling | Value | Past it |
|---|---|---|
| image side | 4096 px | Sixel: pixels past it are dropped; kitty: refused |
| `DCS` / `APC` payload | 16 MiB | aborted, no image (`aborted_dcs` for DCS) |
| waiting for `take_graphics` | 128 MiB of pixels | oldest dropped, its placements released (`graphics_dropped`) |
| live placements | 256 | the oldest is released |
| pixels of placed images | 256 MiB, each image once | the oldest placements are released |

A newer placement over an older one's whole extent releases it at once, unless the older one is on
a higher layer. See [limits](limits.md). Not supported: Sixel background select (`P2`), pixel
aspect, DECSDM (`?80`), XTSMGRAPHICS, `CSI 16 t`, OSC 1337 images.
