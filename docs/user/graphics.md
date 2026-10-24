# Graphics (Sixel)

A program draws an image with Sixel (`DCS q ... ST`). The engine decodes it to RGBA, anchors it to
the cells it covers and tells you when those cells are gone. It never rescales, draws or uploads:
pixels come from `Terminal::take_graphics`, positions from `SnapshotState::placements`, the end of an
image from `VtEvent::GraphicReleased`.

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
        textures.remove(id); // free
    }
}
term.snapshot_update(&mut state, now);
let placement = state.placements()[0];
assert_eq!((placement.cols, placement.rows), (1, 1)); // one 8x16 cell
assert!(textures.contains_key(&placement.id));
assert_eq!(state.rows()[0].cells[0].graphic, Some(placement.id));

term.feed(b"\x1bc", &mut batch, now); // RIS: the grid is empty again
assert!(batch.iter().any(|event| *event == VtEvent::GraphicReleased(placement.id)));
```

## Lifecycle

```text
DCS q ... ST --decode--> GraphicData (RGBA)  --take_graphics--> your texture, keyed by GraphicId
             --place at the cursor--> Placement + cells that name the GraphicId
                 --snapshot_update--> SnapshotPlacement { row, col, cols, rows, pixel_size }
   rows leave the grid / are erased / evicted --> VtEvent::GraphicReleased(id) --> free the texture
```

A placement is an anchor on its top-left cell: it scrolls into history, follows IL / DL / SU / SD and
survives reflow with its rows. It lives while its anchor lives and a row of its extent still carries
an image cell. Text printed over an image cell replaces that cell only.

## Embedder checklist

- Per cycle, in this order: `feed`, then `take_graphics` (upload), then the batch (`GraphicReleased`
  frees), then `snapshot_update`. `take_graphics` is the only drain; each image is handed out once,
  even one released before you took it (the release that follows frees it).
- Call `Terminal::set_cell_pixels(width, height)` on every font or DPI change.
- Key textures by `GraphicId`. Ids start at 1 and are never reused, not even after RIS.
- `GraphicReleased` arrives in the next `feed`, an empty one included, and before `Repaint`.

## Geometry

- Footprint: `ceil(pixels / cell)` per axis against `set_cell_pixels`; when either axis is 0 the
  `VIRTUAL_CELL` (10 x 20) is used for both. Columns are clipped to the screen, rows are not
  clipped at the bottom. The footprint is fixed when the image is placed.
- Cursor: it walks `bands above the last * 6 / cell_height` rows and keeps its column (the DEC
  rule, not "column 0 below the image"). It ends on the image's last row, or below it when the
  payload ends in `-` and the cell height divides the image.
- Raster attributes (`"Pan;Pad;Ph;Pv`) set the image size; data past it is cropped and missing
  data is transparent. Without them the painted extent is the size.
- `CSI 14 t` answers `rows * cell_height ; cols * cell_width` of the effective cell, so a program
  that asks before you call `set_cell_pixels` sees `VIRTUAL_CELL`.

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
let (dx, dy) = state.graphic_offset(id, row.id, 2).expect("the image is live");
assert_eq!((dx, dy), (2, 1)); // draw the texture at cell_origin - (dx, dy) * cell_size
```

- By cells (recommended): for a cell with `graphic: Some(id)`, `graphic_offset(id, row.id, col)`
  gives `(dx, dy)` in cells; position the texture at `cell_origin - (dx, dy) * cell_size`, one image
  pixel per device pixel, clipped to that cell. Cells whose text replaced the image, and rows split
  by IL, then draw correctly. `None` means the image was released: draw nothing.
- By placement (simpler, ignores overwrites): the texture at the placement's top-left cell, clipped to
  `cols x rows`. `placements()` lists both screens, oldest first. Draw text on top of image cells.

## Ceilings

| Ceiling | Value | Past it |
|---|---|---|
| image side | 4096 px | pixels past it are dropped |
| `DCS` payload | 16 MiB | aborted, no image (`aborted_dcs`) |
| waiting for `take_graphics` | 128 MiB of pixels (each image counts at least its struct size) | oldest dropped and released (`graphics_dropped`) |
| live placements | 256 | the oldest is released |

A newer image over an older one's whole extent releases it at once. See [limits](limits.md).

## Not supported

Background select (`P2`), pixel aspect, DECSDM (`?80`), XTSMGRAPHICS, `CSI 16 t`, other image protocols.
