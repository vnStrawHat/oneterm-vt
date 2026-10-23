//! Tier 4: resizing a terminal that holds a full scrollback, every line of which reflows.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use oneterm_vt::{Config, EventBatch, ResizePolicy, Size, Terminal};

use crate::fixtures::FIXTURES;
use crate::perf_history::ResizeCost;
use crate::throughput::{CHUNK, RUNS};

/// Scrollback depths (rows of history) measured.
pub(crate) const RESIZE_DEPTHS: [u32; 3] = [0, 10_000, 100_000];

const SMALL: Size = Size { rows: 24, cols: 80 };
const LARGE: Size = Size {
    rows: 40,
    cols: 100,
};

/// Tier 4: for each depth, the median time to grow 80x24 to 100x40 and to shrink back.
pub(crate) fn measure_resize() -> Vec<ResizeCost> {
    measure_depths(&RESIZE_DEPTHS)
}

fn measure_depths(depths: &[u32]) -> Vec<ResizeCost> {
    depths.iter().map(|depth| measure_depth(*depth)).collect()
}

fn measure_depth(depth: u32) -> ResizeCost {
    let mut term = Terminal::new(
        SMALL,
        Config {
            scrollback_limit: depth,
            ..Config::default()
        },
    );
    let mut batch = EventBatch::new();
    let now = Instant::now();
    // 96 bytes per row of history is more than the 80 the wrapped lines take, so it is full.
    let target = (depth as usize + usize::from(SMALL.rows)) * 96;
    let stream = FIXTURES
        .iter()
        .find(|fixture| fixture.name == "long_lines")
        .map_or_else(Vec::new, |fixture| (fixture.make)(target));
    for chunk in stream.chunks(CHUNK) {
        term.feed(chunk, &mut batch, now);
    }
    let (mut grow, mut shrink) = (Vec::new(), Vec::new());
    for _ in 0..RUNS {
        grow.push(timed_resize(&mut term, LARGE));
        shrink.push(timed_resize(&mut term, SMALL));
    }
    ResizeCost {
        scrollback_rows: depth,
        grow_us: median_us(&mut grow),
        shrink_us: median_us(&mut shrink),
    }
}

fn timed_resize(term: &mut Terminal, size: Size) -> Duration {
    let start = Instant::now();
    let outcome = term.resize(size, ResizePolicy::BottomAnchor);
    let elapsed = start.elapsed();
    std::hint::black_box(outcome);
    elapsed
}

fn median_us(samples: &mut [Duration]) -> f64 {
    samples.sort_unstable();
    samples
        .get(samples.len() / 2)
        .map_or(0.0, |median| median.as_secs_f64() * 1e6)
}

pub(crate) fn render_table(rows: &[ResizeCost]) -> String {
    let mut table = String::from("| Scrollback rows | grow us | shrink us |\n|---:|---:|---:|\n");
    for row in rows {
        // Writing to a String cannot fail.
        let _ = writeln!(
            table,
            "| {} | {:.1} | {:.1} |",
            row.scrollback_rows, row.grow_us, row.shrink_us
        );
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_rows_have_three_depths() {
        assert_eq!(RESIZE_DEPTHS.len(), 3);
        let rows = measure_depths(&[0, 100]);
        assert_eq!(rows.len(), 2);
        for (row, depth) in rows.iter().zip([0, 100]) {
            assert_eq!(row.scrollback_rows, depth);
            assert!(row.grow_us.is_finite() && row.grow_us >= 0.0);
            assert!(row.shrink_us.is_finite() && row.shrink_us >= 0.0);
        }
    }

    #[test]
    fn table_has_a_header_and_one_row_per_depth() {
        let rows = vec![
            ResizeCost {
                scrollback_rows: 0,
                grow_us: 1.25,
                shrink_us: 2.0,
            },
            ResizeCost {
                scrollback_rows: 10_000,
                grow_us: 300.0,
                shrink_us: 400.0,
            },
        ];
        let table = render_table(&rows);
        assert!(table.starts_with("| Scrollback rows | grow us | shrink us |\n"));
        assert!(table.contains("| 10000 | 300.0 | 400.0 |\n"));
        assert_eq!(table.lines().count(), 2 + rows.len());
    }
}
