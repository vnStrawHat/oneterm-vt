# S3 decode buffer becomes the image, S4 id sweeps over the `occ` prefix (graphics)

## S3 root cause: three buffers and two copies per image (`sixel_large`)

`sixel_large`: 596 images of 1024x768 (3 MiB of RGBA each), 56 KB of input each. Profile at
HEAD: `ntdll` 32 % (large blocks: `NtAllocateVirtualMemory` / `NtFreeVirtualMemory`, page
faults), `VCRUNTIME140` 16 % (`memset`, `memcpy`), `dcs_unhook` 20 % (the copy loop of
`finish`, inlined), `paint` 20 %.

- `reserve` grows the decode buffer by doubling: the stride doubles 64 -> 1024 (`widen`
  re-lays the rows into a new buffer each time) and `pixels.resize` reallocates as bands
  arrive, each step a fresh block that is zero-filled and faulted in.
- `finish` allocates `width * height * 4` zeroed bytes and copies the pixels into them one
  4-byte pixel at a time.

Peak per image: decode buffer plus image (up to 64 + 64 MiB for a 4096x4096 raster).

## S3 design (`graphics/sixel.rs`, no `unsafe`)

| Item | Change |
|---|---|
| `declare_raster` | when nothing is painted yet: `pixels = vec![TRANSPARENT; w * h]` (allocated zeroed, no `memset`), `stride = w`. A raster after painting keeps today's path |
| `finish` | re-lay the buffer to `width` pixels per row: `widen(width)` when `width > stride` (unchanged helper), `narrow(width)` when `width < stride`; then `resize(width * height, TRANSPARENT)` (cuts extra rows, pads missing ones), `shrink_to_fit`, `rgba = pixels.into_flattened()` (`Vec<[u8; 4]>` -> `Vec<u8>`, std since 1.80) |
| `narrow(stride)` (new, private) | in place: `copy_within` row `r` from `r * old` to `r * new` for `r >= 1` (forward, never overlapping wrongly), then `truncate(rows * new)` |

- The image equals today's for every input: rows cut or padded to `height`, columns cut to
  `min(width, stride)`, untouched pixels transparent (a proptest pins it against the old copy
  loop, tests.md).
- Ceilings: one buffer of at most 64 MiB per image instead of two; limits.md's "image decode
  buffer 64 MiB" stays true (now also as peak). A raster declared and never painted
  allocates its buffer at the declaration instead of in `finish` (same size, same ceiling).
- `GraphicData.rgba` capacity equals its length (`shrink_to_fit`), so the 128 MiB pending
  queue accounting (by `len`) stays honest.

Technique: rio sizes its picture from the raster attributes when nothing is painted yet
(`sixel.rs`, "If there is no current picture, creates a new one"); it still converts to RGBA
in `finish`. Here the RGBA decode buffer itself is handed over, so there is no second pass.

## S4 root cause: the extras sweep reads every cell of every flagged row (`sixel`)

`sixel` puts one 12x12 image per line: every row carries `HAS_EXTRAS`, every image interns
one extras entry, so `sweep_extras_if_due` runs every 4,096 images and walks 10,045 rows x
160 cells: `collect_extras_ids` 15 % of the fixture.

## S4 design (`grid/screen.rs`)

`collect_extras_ids` and `collect_style_ids`: for a flagged row, read `cells[..occ]` plus the
last cell instead of every cell. The row invariant (`row.rs`: every cell at `col >= occ`
equals the last cell) makes that exactly the same id set. A sixel row has `occ` 2, a plain
erased row 0.

```rust
let cells = row.cells();
let used = usize::from(row.header().occ).min(cells.len());
for cell in cells.get(..used).unwrap_or_default().iter().chain(cells.last()) { ... }
```

## Expected (pinned MiB/s, evidence.md)

| Fixture | before (S2) | after S3 | after S4 |
|---|---:|---:|---:|
| `sixel_large` | 25.7 | 42.0 | 50.3 |
| `sixel` (S3: best of 3) | 46.1 | 54.0 | 64.9 |

## Left (measured, evidence.md "What stays behind")

Profile of `sixel` after S1-S6: `dcs_unhook` 28 % (mostly `release_covered`, which reads the
anchor of every live placement, 256 at the cap, for each new image), `paint` 18 %,
system DLLs 16 % (one image allocation and free each), `dcs_put` 13 % (per byte through the
decoder's phase match).
