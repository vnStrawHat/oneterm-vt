# Colour scheme: dark or light

Programs choose their own colours by whether the terminal is dark or light. They ask with
`CSI ? 996 n`, and with `?2031` they ask to be told when it changes. Only you know the answer:
`Terminal::set_color_scheme` tells the engine, and hands you the report to write.

## Example: answer the query, report a theme switch

```rust
use std::time::Instant;
use oneterm_vt::{ColorScheme, Config, EventBatch, Mode, Size, Terminal, VtEvent};

let mut term = Terminal::new(Size { rows: 24, cols: 80 }, Config::default());
let mut batch = EventBatch::new();

// At start-up, say which scheme your colours are. Nobody asked for updates: nothing to write.
assert_eq!(term.set_color_scheme(ColorScheme::Dark), None);

// The program asks, then subscribes to changes.
term.feed(b"\x1b[?996n\x1b[?2031h", &mut batch, Instant::now());
let VtEvent::Reply(span) = &batch.events()[0] else { panic!("an answer") };
assert_eq!(batch.bytes(*span), b"\x1b[?997;1n"); // dark
assert!(term.mode(Mode::ColorSchemeUpdates));

// The user switches to a light theme: write what comes back to the program.
assert_eq!(term.set_color_scheme(ColorScheme::Light), Some(&b"\x1b[?997;2n"[..]));
// The same scheme again is no change.
assert_eq!(term.set_color_scheme(ColorScheme::Light), None);
```

## Rules

| Sequence | What the engine does |
|---|---|
| `CSI ? 996 n` | `Reply` with `CSI ? 997 ; 1 n` (dark) or `CSI ? 997 ; 2 n` (light), whatever `?2031` is |
| `CSI ? 2031 h` / `l` | stores the mode (`Mode::ColorSchemeUpdates`); DECRQM answers it; says nothing by itself |
| RIS | turns `?2031` off, keeps the scheme you told |
| DECSTR | keeps both |

- `set_color_scheme` returns the report only when the scheme differs from the last one you told
  **and** `?2031` is set. The first call counts as a change: a program that subscribed before you
  named a scheme hears it then.
- Until the first call, `CSI ? 996 n` gets no answer and is counted in
  `FeedStats::unhandled_sequences`. The engine does not guess the scheme from the theme colours;
  a program without an answer falls back to asking for the background colour (OSC 11).
- The report bytes are part of the API, like every reply.

## What you owe

- Call `set_color_scheme` once when the terminal is created and again whenever your theme changes,
  next to `Terminal::set_theme_colors`. Calling it with the same value is free.
- Write the returned bytes to the program, in order with the replies and the input you send it.
- With several terminals (tabs, panes), call it on each one: every program is told separately.
