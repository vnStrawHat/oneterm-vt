//! Tier 3: what a renderer pays per frame. Output arrives between frames (untimed), then one
//! timed `snapshot_update` per frame on a state reused across frames, so damage tracking is
//! what gets measured, not the cost of a fresh `Full` rebuild.

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::{Duration, Instant};

use oneterm_vt::{Config, EventBatch, Size, SnapshotState, Terminal};

use crate::fixtures::{COLUMNS, FIXTURES, Fixture, LINES};
use crate::perf_history::FrameCost;
use crate::throughput::{CHUNK, RUNS};

/// Frames per cycle unless `--frames` says otherwise.
pub(crate) const DEFAULT_FRAMES: usize = 600;
/// Output bytes that arrive between two paints.
pub(crate) const FRAME_BYTES: usize = 512;

/// What one cycle measured.
struct Sample {
    us_per_frame: f64,
    cells: u64,
}

/// Tier 3: for each of `fixtures`, the median microseconds per `snapshot_update` over
/// `frames` frames, and the cells copied per frame.
///
/// Cycles are interleaved across fixtures like tier 2's, and cycle 0 is a discarded warm-up.
pub(crate) fn measure_render(frames: usize, fixtures: &[&Fixture]) -> Vec<FrameCost> {
    let mut samples: Vec<Vec<Sample>> = fixtures.iter().map(|_| Vec::new()).collect();
    for cycle in 0..=RUNS {
        for (fixture, kept) in fixtures.iter().zip(samples.iter_mut()) {
            let stream = (fixture.make)(frames * FRAME_BYTES + 1024);
            let sample = run_cycle(&stream, frames);
            if cycle > 0 {
                kept.push(sample);
            }
        }
    }
    fixtures
        .iter()
        .zip(samples)
        .map(|(fixture, kept)| summarize(fixture.name, kept))
        .collect()
}

/// One cycle: prime a terminal so every viewport cell holds content, then per frame feed the
/// next slice of `stream` untimed and time one `snapshot_update`. A `stream` that has run out
/// feeds empty slices, which change nothing.
fn run_cycle(stream: &[u8], frames: usize) -> Sample {
    let size = Size {
        rows: LINES as u16,
        cols: COLUMNS as u16,
    };
    let mut term = Terminal::new(size, Config::default());
    let mut batch = EventBatch::new();
    let mut state = SnapshotState::new();
    let now = Instant::now();

    let prime = FIXTURES
        .iter()
        .find(|fixture| fixture.name == "dense_cells")
        .map_or_else(Vec::new, |fixture| (fixture.make)(COLUMNS * LINES * 20));
    for chunk in prime.chunks(CHUNK) {
        term.feed(chunk, &mut batch, now);
    }
    term.snapshot_update(&mut state, now);

    let copied_before = state.rows_copied();
    let mut spent = Duration::ZERO;
    for frame in 0..frames {
        let rest = stream.get(frame * FRAME_BYTES..).unwrap_or_default();
        let bytes = rest.get(..FRAME_BYTES).unwrap_or(rest);
        term.feed(bytes, &mut batch, now);
        // Images are drained after every feed, outside the timed update.
        black_box(term.take_graphics());
        let start = Instant::now();
        black_box(term.snapshot_update(&mut state, now));
        spent += start.elapsed();
    }
    let copied = state.rows_copied().saturating_sub(copied_before);
    black_box(&state);
    Sample {
        us_per_frame: spent.as_secs_f64() * 1e6 / frames.max(1) as f64,
        // Counted outside the timer; a constant viewport area would say nothing about damage.
        cells: copied * COLUMNS as u64 / frames.max(1) as u64,
    }
}

/// The median cycle by time; its `cells` stand for the fixture (the count is deterministic).
fn summarize(fixture: &str, mut samples: Vec<Sample>) -> FrameCost {
    samples.sort_by(|a, b| a.us_per_frame.total_cmp(&b.us_per_frame));
    let median = samples.get(samples.len() / 2);
    FrameCost {
        fixture: fixture.to_owned(),
        us_per_frame: median.map_or(0.0, |sample| sample.us_per_frame),
        cells: median.map_or(0, |sample| sample.cells),
    }
}

pub(crate) fn render_table(rows: &[FrameCost]) -> String {
    let mut table = String::from("| Fixture | us/frame | cells/frame |\n|---|---:|---:|\n");
    for row in rows {
        // Writing to a String cannot fail.
        let _ = writeln!(
            table,
            "| `{}` | {:.1} | {} |",
            row.fixture, row.us_per_frame, row.cells
        );
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_runs_every_fixture() {
        let rows = measure_render(4, &crate::fixtures::select(None).unwrap());
        let names: Vec<&str> = rows.iter().map(|row| row.fixture.as_str()).collect();
        let expected: Vec<&str> = FIXTURES.iter().map(|fixture| fixture.name).collect();
        assert_eq!(names.len(), 10);
        assert_eq!(names, expected);
    }

    #[test]
    fn render_runs_selected_only() {
        let heavy_sgr = crate::fixtures::select(Some("heavy_sgr")).unwrap();
        let rows = measure_render(2, &heavy_sgr);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fixture, "heavy_sgr");
    }

    #[test]
    fn render_values_are_sane() {
        for row in measure_render(4, &crate::fixtures::select(None).unwrap()) {
            assert!(
                row.us_per_frame.is_finite() && row.us_per_frame >= 0.0,
                "{}",
                row.fixture
            );
            assert!(row.cells <= (COLUMNS * LINES) as u64, "{}", row.fixture);
        }
    }

    #[test]
    fn idle_frames_copy_nothing() {
        assert_eq!(run_cycle(&[], 50).cells, 0);
    }

    #[test]
    fn busy_frames_copy_something() {
        let fixture = FIXTURES
            .iter()
            .find(|fixture| fixture.name == "plain_ascii")
            .expect("plain_ascii exists");
        let stream = (fixture.make)(50 * FRAME_BYTES + 1024);
        assert!(run_cycle(&stream, 50).cells > 0);
    }

    #[test]
    fn summarize_takes_the_median_cycle() {
        let sample = |us_per_frame, cells| Sample {
            us_per_frame,
            cells,
        };
        let cost = summarize("f", vec![sample(3.0, 30), sample(1.0, 10), sample(2.0, 20)]);
        assert_eq!(cost.us_per_frame, 2.0);
        assert_eq!(cost.cells, 20);
        assert_eq!(summarize("f", Vec::new()).cells, 0);
    }

    #[test]
    fn table_has_a_header_and_one_row_per_fixture() {
        let rows = vec![FrameCost {
            fixture: "plain_ascii".into(),
            us_per_frame: 3.1,
            cells: 160,
        }];
        let table = render_table(&rows);
        assert!(table.starts_with("| Fixture | us/frame | cells/frame |\n|---|---:|---:|\n"));
        assert!(table.contains("| `plain_ascii` | 3.1 | 160 |\n"));
    }
}
