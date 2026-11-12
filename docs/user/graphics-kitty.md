# Kitty graphics protocol

With the `kitty-graphics` feature (on by default) the engine speaks the image protocol of
`icat`, `chafa`, `timg`, `yazi` and `image.nvim`: `APC G <keys> ; <base64 payload> ST`. You draw
its images exactly as [Sixel ones](graphics.md); this chapter says what a program can ask for.

## Example: transmit once, place twice

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, Size, SnapshotState, Terminal, VtEvent};

let now = Instant::now();
let mut term = Terminal::new(Size { rows: 5, cols: 20 }, Config::default());
let (mut batch, mut state) = (EventBatch::new(), SnapshotState::new());
let replies = |batch: &EventBatch| -> Vec<u8> {
    let spans = batch.iter().filter_map(|event| match event {
        VtEvent::Reply(span) => Some(batch.bytes(*span)),
        _ => None,
    });
    spans.flatten().copied().collect()
};

// a=t: store image 7, one RGB pixel (f=24, 1 x 1). "/wAA" is base64 for ff 00 00.
term.feed(b"\x1b_Ga=t,i=7,f=24,s=1,v=1;/wAA\x1b\\", &mut batch, now);
assert_eq!(replies(&batch), b"\x1b_Gi=7;OK\x1b\\"); // write replies back, as always
assert!(term.take_graphics().is_empty()); // nothing is shown yet

// a=p: place it twice; the second one stretched over 4 x 2 cells, under the text.
term.feed(b"\x1b_Ga=p,i=7\x1b\\\x1b_Ga=p,i=7,c=4,r=2,z=-1\x1b\\", &mut batch, now);
let images = term.take_graphics();
assert_eq!(images.len(), 1); // one image, handed out once
assert_eq!(images[0].rgba, [255, 0, 0, 255]);

term.snapshot_update(&mut state, now);
let [first, second] = state.placements() else { panic!("two placements") };
assert_eq!((first.image, second.image), (images[0].id, images[0].id));
assert_eq!((first.display_size, second.display_size), ((1, 1), (40, 40))); // 10 x 20 cells
assert_eq!((second.cols, second.rows, second.z_index), (4, 2, -1));

// a=d,d=I: delete the placements of image 7 and forget the image.
term.feed(b"\x1b_Ga=d,d=I,i=7\x1b\\", &mut batch, now);
let released = batch.iter().filter(|e| matches!(e, VtEvent::GraphicReleased(_))).count();
assert_eq!(released, 3); // two placements, then the image
```

## What a program can send

| Action | Does |
|---|---|
| `a=q` | decodes the data and answers, storing nothing: how programs detect the protocol |
| `a=t` | stores an image under `i` (or under a number `I`; the reply carries the id) |
| `a=T` | stores and places it; without `i` and `I` the image is shown once and not stored |
| `a=p` | places a stored image: `x y w h` source rectangle, `X Y` offset in the cell, `c r` cells to stretch over, `z` layer, `C=1` to leave the cursor, `p` a placement id that moves on reuse |
| `a=d` | deletes placements: `d=` `a i n c p q x y z r`; the upper-case letter also forgets images left without a placement |

- Data: `f=24` RGB, `f=32` RGBA (with `s`, `v`), `f=100` PNG; `o=z` for zlib; `m=1` for chunks.
- Replies: `APC G i=..[,I=..][,p=..] ; OK` or `; ECODE:reason`, as `VtEvent::Reply`. None without
  `i` or `I`, for `a=d`, or as `q=1` (no `OK`) and `q=2` (nothing) ask.
- The cursor ends on the last row of the image, right of it; past the last column it moves to the
  start of the next line. A tall image scrolls the screen and moves with its rows.

## What is different from kitty

- **Cells own the image.** Text printed over an image cell takes that cell, and a placement
  whose rows are erased or evicted is released. Kitty keeps images in a layer above the grid.
- **Direct data only** (`t=d`). File, temporary-file and shared-memory transmission are refused
  with `EINVAL`, so programs fall back to direct data. The engine never opens a file a stream names.
- **Refused:** animation (`a=f`, `a=a`, `a=c`), Unicode placeholders (`U=1`), relative placements
  (`P`, `Q`). Each answers `EINVAL` and counts in `FeedStats::unhandled_sequences`.
- Images are stored per screen: the alternate screen's image 7 is not the primary screen's.
- A newer placement that covers an older one on the same or a lower layer releases it.

## Ceilings

| Ceiling | Value | Past it |
|---|---|---|
| image side | 4096 px | `EINVAL` |
| one image's data, decoded | 64 MiB | `EFBIG`; a zlib stream is cut at the image size |
| one `APC` string | 16 MiB | aborted; a chunked load in flight is dropped |
| stored images | 128 MiB of pixels and 1,024 images, both screens | the oldest is forgotten; its placements stay |
| rows of one placement | the screen height, or `4096 / cell height` if more | clamped |

Stored images share their pixels with the textures you hold: what you keep is bounded by this
store plus the placed-image budget of [graphics](graphics.md). Decoding runs inside `feed`; a
zlib or PNG payload can expand a thousandfold, up to the 64 MiB cap, so a hostile stream can make
`feed` slow but not unbounded.

## Without the feature

`default-features = false` (then name the features you want) drops `png` and `miniz_oxide`;
`APC G` is then consumed like any other APC and counted in `unhandled_sequences`. The
`Placement` fields stay, with their Sixel values.
