//! A terminal with no window: feed one buffer, print the events, print the screen as text.
//!
//! Uses engine items only (no `pty`), so it also builds with `--no-default-features`.
//!
//! Run: `cargo run -p oneterm-vt --example headless`

use std::io::{self, Write};
use std::time::Instant;

use oneterm_vt::{
    Config, EventBatch, OscRoute, OscRoutes, Size, SnapshotContent, SnapshotRow, SnapshotState,
    Terminal, VtEvent,
};

/// Title, working directory, an OSC the embedder routes to itself, styled text, a cursor move and a
/// device-attributes query.
const INPUT: &[u8] = b"\x1b]0;headless example\x07\
    \x1b]7;file://host/tmp\x07\
    \x1b]7501;hello\x07\
    \x1b[1;32mhello\x1b[0m world\
    \x1b[3;3Hrow three\
    \x1b[c";

fn main() -> io::Result<()> {
    let mut out = io::stdout().lock();

    let mut osc_routes = OscRoutes::new();
    osc_routes.route(7501, OscRoute::Forward);
    let config = Config {
        osc_routes,
        product_name: Some("headless-example(0.1.0)".into()),
        ..Config::default()
    };

    let mut term = Terminal::new(Size { rows: 4, cols: 32 }, config);
    let mut batch = EventBatch::new();
    let stats = term.feed(INPUT, &mut batch, Instant::now());

    // Spans index their batch: resolve them before the next `feed`.
    writeln!(out, "-- events --")?;
    for event in batch.iter() {
        match event {
            VtEvent::Title(span) => writeln!(out, "title      {:?}", batch.str(*span))?,
            VtEvent::Cwd { host, path } => {
                writeln!(
                    out,
                    "cwd        host {:?}, path {:?}",
                    batch.str(*host),
                    batch.str(*path)
                )?;
            }
            VtEvent::Osc { code, params, .. } => {
                let rest: Vec<String> = batch
                    .params(*params)
                    .skip(1) // parameter 0 is the code text
                    .map(|param| String::from_utf8_lossy(param).into_owned())
                    .collect();
                writeln!(out, "osc {code}   {:?}", rest.join(";"))?;
            }
            // A real embedder writes these bytes to the program, verbatim and in order.
            VtEvent::Reply(span) => {
                writeln!(out, "reply      {}", batch.bytes(*span).escape_ascii())?;
            }
            other => writeln!(out, "{other:?}")?,
        }
    }
    writeln!(
        out,
        "{} bytes fed, {} unhandled sequences",
        stats.bytes, stats.unhandled_sequences
    )?;

    // One reused state: the next update is a diff instead of a full copy.
    let mut state = SnapshotState::new();
    let update = term.snapshot_update(&mut state, Instant::now());
    writeln!(out, "-- screen ({update:?}) --")?;
    for row in state.rows() {
        writeln!(out, "|{}|", row_text(row))?;
    }
    Ok(())
}

/// A snapshot row as plain text: wide-glyph spacer cells are skipped, clusters expanded.
fn row_text(row: &SnapshotRow) -> String {
    let mut text = String::new();
    for cell in &row.cells {
        if cell.width.is_spacer() {
            continue;
        }
        match cell.content {
            SnapshotContent::Scalar(c) => text.push(c),
            SnapshotContent::Cluster { start, len } => text.extend(row.cluster(start, len)),
        }
    }
    text
}
