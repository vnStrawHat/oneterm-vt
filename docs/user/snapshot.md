# Snapshots

A `SnapshotState` is one consumer's view of one terminal: the viewport rows, the cursor, the
selection and the modes, copied out under your lock. `Terminal::snapshot_update(&mut state, now)`
copies only the rows written since that state last looked (damage is a per-row stamp checked against
the state's `Watermark`), so an idle terminal costs nothing and nothing is cleared on the engine's
side. Rows hold values (resolved styles, cluster scalars, link strings), so drawing needs no terminal.

## Example: keep a drawn cache in step

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, Size, SnapshotContent, SnapshotRow, SnapshotState};
use oneterm_vt::{SnapshotUpdate, Terminal};

fn text(row: &SnapshotRow) -> String {
    let mut line = String::new();
    for cell in row.cells.iter().filter(|cell| !cell.width.is_spacer()) {
        match cell.content {
            SnapshotContent::Scalar(c) => line.push(c),
            SnapshotContent::Cluster { start, len } => line.extend(row.cluster(start, len)),
        }
    }
    line.trim_end().to_owned()
}

let now = Instant::now();
let mut term = Terminal::new(Size { rows: 3, cols: 8 }, Config::default());
let (mut batch, mut state) = (EventBatch::new(), SnapshotState::new());
let mut cache: Vec<String> = Vec::new(); // what you drew, one entry per viewport row

for output in [&b"one\r\ntwo\r\nthree"[..], b"\r\nfour"] {
    term.feed(output, &mut batch, now);
    match term.snapshot_update(&mut state, now) {
        SnapshotUpdate::Unchanged => {}
        SnapshotUpdate::Full => cache = state.rows().iter().map(text).collect(),
        SnapshotUpdate::Partial { scrolled } => {
            let by = scrolled.unsigned_abs() as usize;
            if scrolled > 0 {
                cache.rotate_left(by); // content moved up
            } else {
                cache.rotate_right(by);
            }
            for &index in state.changed() {
                cache[usize::from(index)] = text(&state.rows()[usize::from(index)]);
            }
        }
    }
}
assert_eq!(cache, ["two", "three", "four"]);
```

## The three results

| Result | Meaning | You |
|---|---|---|
| `Unchanged` | no row, cursor, selection or mode changed (or `?2026` holds the frame) | nothing |
| `Partial { scrolled }` | some rows, or only the cursor, selection or modes, changed | shift the cache by `scrolled` rows (positive: content moved up), redraw `changed()` |
| `Full` | first update, resize, screen switch, big scroll, colour or theme change, `invalidate()` | redraw every row |

`changed()` lists the viewport indices copied, ascending. `cursor()`, `selection()` and `modes()`
refresh on every update. A row's `id` follows content. `rows_copied()` stays flat across idle updates.

## Rows and styles

Skip `cell.width.is_spacer()` cells: the wide cell on their left draws them. `cell.hyperlink` is
resolved with `state.hyperlink(id)`. `cell.graphic` and `state.placements()` carry Sixel images
(see [graphics](graphics.md)); a placement change counts as a `Partial` update.

## Colours: `Palette`, theme and program overrides

```rust
use std::time::Instant;
use oneterm_vt::{Color, Config, EventBatch, Palette, Rgb, Size, SnapshotState, Terminal};

let now = Instant::now();
let mut term = Terminal::new(Size { rows: 2, cols: 8 }, Config::default());
let (mut batch, mut state) = (EventBatch::new(), SnapshotState::new());

let mut theme = Palette::new();
theme.background = Rgb { r: 0, g: 0, b: 0 };
term.set_theme_colors(&theme); // a new theme makes the next update Full; the same one is free
term.feed(b"\x1b]4;1;rgb:ff/00/00\x07\x1b[31mx\x1b[32my", &mut batch, now);

term.snapshot_update(&mut state, now);
state.map_colors(&term.palette()); // theme + program overrides, Rgb everywhere
let row = &state.rows()[0];
let red = row.style_of(&row.cells[0]).fg;
assert_eq!(red, Color::Rgb(Rgb { r: 255, g: 0, b: 0 })); // OSC 4 wins over the theme
let green = row.style_of(&row.cells[1]).fg;
assert_eq!(green, Color::Rgb(theme.indexed[2]));
```

- Precedence per slot: a program's override (OSC 4, 10, 11, 12) wins; resetting it (OSC 104, 110-112,
  RIS) returns the slot to the theme. Selection colours (OSC 17, 19) have no `Palette` slot: read them
  with `Terminal::color`.
- Call `state.map_colors(&term.palette())` after every update: it remaps every row when the colour
  epoch changed, else only `changed()`. Attributes stay yours: inverse (SGR 7), bold-as-bright, dim
  and reverse video (`ModeSnapshot::reverse_video`).

## Modes and the mouse

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, MouseEncoding, MouseReporting, Size, Terminal};

let mut term = Terminal::new(Size { rows: 2, cols: 8 }, Config::default());
term.feed(b"\x1b[?1002h\x1b[?1006h\x1b[?2004h", &mut EventBatch::new(), Instant::now());
let modes = term.mode_snapshot(); // the same value `state.modes()` holds after an update
assert!(modes.bracketed_paste && modes.show_cursor && !modes.alt_screen);
let mouse = modes.mouse.expect("reporting is on");
assert_eq!(mouse.reporting, MouseReporting::ButtonEvent);
assert_eq!(mouse.encoding, MouseEncoding::Sgr);
assert_eq!(term.mouse_reporting(), modes.mouse);
```

`ModeSnapshot` is `#[non_exhaustive]`: start from `default()` or `mode_snapshot()` and assign fields.
Reporting (`?9`, `?1000`, `?1002`, `?1003`) and encoding (`?1005`, `?1006`, `?1015`) are separate; an
encoding without reporting gives `None` and applies once reporting is on. `SnapshotCursor.row` is
`None` while the viewport is scrolled back past the cursor.
