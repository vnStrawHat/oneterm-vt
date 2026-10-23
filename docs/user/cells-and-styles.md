# Cells and styles

`oneterm_vt::Cell` is what one grid position stores: 8 bytes, no pointer. Anything rare (style,
hyperlink, image, long cluster) is an id into the terminal's `oneterm_vt::intern::Interner`, so
uniform text stores one style however many cells use it. `scalar_width` and `cluster_width` say
how many columns text takes. Input is untrusted: every table has a cap, and past it the value
degrades to a default and a counter grows. Nothing panics or returns an error.

## Example: a bold red `x`

```rust
use oneterm_vt::intern::Interner;
use oneterm_vt::{Attrs, Cell, CellContent, Color, Rgb, Style};

let mut interner = Interner::default();
let style = Style {
    fg: Color::Rgb(Rgb { r: 255, g: 0, b: 0 }),
    attrs: Attrs::BOLD | Attrs::UNDERLINE,
    ..Style::DEFAULT
};
let id = interner.style(style);
let cell = Cell::EMPTY.with_content(CellContent::Scalar('x')).with_style(id);

assert_eq!(cell.content(), CellContent::Scalar('x'));
assert_eq!(*interner.resolve_style(cell.style_id()), style);
assert_eq!(interner.style(style), id); // equal styles share one id
```

## What a cell is

- Fields: `content()` (a scalar or a grapheme id), `width()` (`CellWidth`: narrow, wide, or one
  of two spacer roles), `semantic()` (OSC 133 zone), `is_protected()`, `style_id()` and
  `extras_id()`. Each has a `with_*` copy-setter that touches only its own field.
- A blank is `Cell::EMPTY`; `' '` and `'\0'` both store as that blank, so `==` is exact.
- A renderer resolves `style_id()`, `extras_id()` and grapheme ids through the terminal's
  `Interner` while it has the terminal borrowed. Do not keep these ids across a feed: a
  grapheme sweep renumbers grapheme ids.
- A `Color` is what the stream named (`Named`, `Palette`, `Rgb`). Turning it into pixels,
  with your theme and OSC 4/10/11 overrides, is your job.

## Example: a grapheme cluster

A cluster of several scalars lives in the arena; the cell holds its id.

```rust
use oneterm_vt::intern::Interner;
use oneterm_vt::{Cell, CellContent};

let mut interner = Interner::default();
let id = interner.grapheme("e\u{301}");
let cell = Cell::EMPTY.with_content(CellContent::Grapheme(id));

assert_eq!(interner.resolve_grapheme(id), "e\u{301}");
assert_eq!(cell.text_char(&interner.graphemes), 'e');
assert_eq!(interner.grapheme("e\u{301}"), id); // deduplicated
```

## Widths

- `scalar_width(char)` is 0, 1 or 2. 0 attaches to the previous cell (combining marks, joiners,
  variation selectors, controls); 2 is East Asian Wide or Fullwidth; ambiguous is 1.
- `cluster_width(&str)` measures the first grapheme cluster. A base plus VS16 is 2, VS15 on an
  emoji base is 1, an emoji with a skin tone or a joined sequence is 2.
- A terminal without grapheme clustering (the default) sums `scalar_width` per scalar; with
  mode `?2027` it measures whole clusters with `cluster_width`, one cell each.

```rust
use oneterm_vt::{cluster_width, scalar_width};

assert_eq!(scalar_width('\u{6F22}'), 2);
assert_eq!(scalar_width('\u{B1}'), 1);
assert_eq!(scalar_width('\u{301}'), 0);
assert_eq!(cluster_width("\u{1F44D}\u{1F3FD}"), 2);
assert_eq!(cluster_width("\u{2764}"), 1);
assert_eq!(cluster_width("\u{2764}\u{FE0F}"), 2);
assert_eq!(cluster_width("\u{231A}\u{FE0E}"), 1);
```

## Sweeping graphemes

Unique clusters are the one growth with no natural reuse, so the arena is collected. Between
sequences, when `needs_grapheme_sweep()` is true, call `graphemes.sweep(live)` with every
grapheme id still stored, then rewrite each stored id through the returned `GraphemeRemap`.

```rust
use oneterm_vt::intern::{GraphemeId, Interner};

let mut interner = Interner::default();
let keep = interner.grapheme("a\u{301}");
let drop = interner.grapheme("o\u{308}");
let remap = interner.graphemes.sweep([keep]);

assert_eq!(interner.resolve_grapheme(remap.get(keep)), "a\u{301}");
assert_eq!(remap.get(drop), GraphemeId(0)); // not kept: resolves to a space
```

## Limits

| Table | Cap | Past the cap |
|---|---|---|
| styles, extras | 65,536 entries each | new value gets id 0 (default); `exhausted()` grows |
| grapheme cluster | 16 scalars | cut at 16; `truncated()` grows |
| grapheme arena | 262,144 clusters or 1,048,576 scalars | new cluster gets id 0 (a space); `exhausted()` grows |
| hyperlinks | 65,536 links or 4 MiB of id and URI text | `intern` returns `None`: print the text, drop the link |

Ids already issued never change meaning, except grapheme ids at a sweep. The counters only
grow; compare them before and after a feed to see what was dropped.
