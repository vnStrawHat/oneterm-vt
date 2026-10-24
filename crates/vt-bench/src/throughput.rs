//! Tiers 1 and 2: the parser alone, then the whole terminal, fed fixtures in PTY-sized chunks.

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::{Duration, Instant};

use oneterm_vt::parser::{Dispatch, Parser};
use oneterm_vt::{Config, EventBatch, Size, Terminal};

use crate::fixtures::{COLUMNS, FIXTURES, LINES};
use crate::perf_history::Throughput;

/// Measured cycles per fixture; cycle 0 is a discarded warm-up.
pub(crate) const RUNS: usize = 5;
/// Bytes per `advance`, like a PTY read.
pub(crate) const CHUNK: usize = 64 * 1024;

/// Receives text and controls and drops them, so the parser's own cost is what gets measured.
struct NullDispatch;

impl Dispatch for NullDispatch {
    fn print_str(&mut self, _text: &str) {}
    fn execute(&mut self, _byte: u8) {}
}

/// Runs `pass` on every fixture for `RUNS + 1` interleaved cycles and summarizes each. The stream is regenerated per cycle (untimed) so ten fixtures are never
/// resident at once.
pub(crate) fn measure_tier(mib: usize, mut pass: impl FnMut(&[u8]) -> Duration) -> Vec<Throughput> {
    let fixtures = &FIXTURES;
    let mut results: Vec<(usize, Vec<Duration>)> = vec![(0, Vec::new()); fixtures.len()];
    for cycle in 0..=RUNS {
        for (fixture, (len, samples)) in fixtures.iter().zip(results.iter_mut()) {
            let bytes = (fixture.make)(mib << 20);
            let elapsed = pass(&bytes);
            if cycle > 0 {
                *len = bytes.len();
                samples.push(elapsed);
            }
        }
    }
    fixtures
        .iter()
        .zip(results.iter_mut())
        .map(|(fixture, (len, samples))| summarize(fixture.name, *len, samples))
        .collect()
}

/// Times one whole terminal over `bytes`; stream generation is not part of the measurement.
pub(crate) fn grid_pass(bytes: &[u8]) -> Duration {
    let size = Size {
        rows: LINES as u16,
        cols: COLUMNS as u16,
    };
    let mut term = Terminal::new(size, Config::default());
    let mut batch = EventBatch::new();
    let now = Instant::now();
    let start = Instant::now();
    for chunk in bytes.chunks(CHUNK) {
        term.feed(black_box(chunk), &mut batch, now);
        // An embedder drains the images after every feed; for most fixtures this is empty.
        black_box(term.take_graphics());
    }
    let elapsed = start.elapsed();
    black_box(&term);
    elapsed
}

/// Times one parser over `bytes`; stream generation is not part of the measurement.
pub(crate) fn parser_pass(bytes: &[u8]) -> Duration {
    let mut parser = Parser::new();
    let mut sink = NullDispatch;
    let start = Instant::now();
    for chunk in bytes.chunks(CHUNK) {
        parser.advance(&mut sink, black_box(chunk));
    }
    let elapsed = start.elapsed();
    black_box(&parser);
    elapsed
}

fn summarize(fixture: &str, len: usize, samples: &mut [Duration]) -> Throughput {
    samples.sort_unstable();
    let rate = |duration: Duration| {
        let seconds = duration.max(Duration::from_nanos(1)).as_secs_f64();
        len as f64 / (1u64 << 20) as f64 / seconds
    };
    let median = samples.get(samples.len() / 2).copied().unwrap_or_default();
    let fastest = samples.first().copied().unwrap_or_default();
    let slowest = samples.last().copied().unwrap_or_default();
    let mib_per_second = rate(median);
    Throughput {
        fixture: fixture.to_owned(),
        mib_per_second,
        ns_per_byte: median.as_nanos() as f64 / len.max(1) as f64,
        spread_percent: (rate(fastest) - rate(slowest)) / mib_per_second * 100.0,
    }
}

pub(crate) fn render_table(rows: &[Throughput]) -> String {
    let mut table = String::from("| Fixture | MiB/s | ns/B | spread |\n|---|---:|---:|---:|\n");
    for row in rows {
        // Writing to a String cannot fail.
        let _ = writeln!(
            table,
            "| `{}` | {:.1} | {:.2} | {:.0}% |",
            row.fixture, row.mib_per_second, row.ns_per_byte, row.spread_percent
        );
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn summarize_reports_median_rate_and_spread() {
        let mut samples = [ms(5), ms(1), ms(3), ms(2), ms(4)];
        let result = summarize("f", 1 << 20, &mut samples);
        assert!((result.mib_per_second - 333.3).abs() < 0.1);
        assert!((result.ns_per_byte - 2.861).abs() < 0.001);
        assert!((result.spread_percent - 240.0).abs() < 0.1);
    }

    #[test]
    fn summarize_survives_a_zero_duration() {
        let mut samples = [Duration::ZERO; 3];
        let result = summarize("f", 1 << 20, &mut samples);
        assert!(result.mib_per_second.is_finite());
        assert!(result.spread_percent.is_finite());
    }

    #[test]
    fn measure_tier_runs_every_fixture_every_cycle() {
        let mut passes = 0;
        let rows = measure_tier(1, |_| {
            passes += 1;
            ms(1)
        });
        assert_eq!(passes, FIXTURES.len() * (RUNS + 1));
        assert_eq!(rows.len(), FIXTURES.len());
    }

    #[test]
    fn tier2_runs_every_fixture() {
        let rows = measure_tier(1, |_| ms(1));
        let names: Vec<&str> = rows.iter().map(|row| row.fixture.as_str()).collect();
        let expected: Vec<&str> = FIXTURES.iter().map(|fixture| fixture.name).collect();
        assert_eq!(names.len(), 10);
        assert_eq!(names, expected);
    }

    #[test]
    fn sixel_fixture_places_an_image_per_unit() {
        let fixture = FIXTURES
            .iter()
            .find(|fixture| fixture.name == "sixel")
            .expect("sixel exists");
        let bytes = (fixture.make)(1 << 16);
        let size = Size {
            rows: LINES as u16,
            cols: COLUMNS as u16,
        };
        let mut term = Terminal::new(size, Config::default());
        let mut batch = EventBatch::new();
        let mut images = 0;
        for chunk in bytes.chunks(CHUNK) {
            term.feed(chunk, &mut batch, Instant::now());
            images += term.take_graphics().len();
        }
        assert!(images > 0);
        assert_eq!(images as u64, term.lines_produced());
        assert!(grid_pass(&bytes) > Duration::ZERO);
    }

    #[test]
    fn grid_pass_feeds_every_byte() {
        let fixture = FIXTURES
            .iter()
            .find(|fixture| fixture.name == "plain_ascii")
            .expect("plain_ascii exists");
        let bytes = (fixture.make)(1 << 20);
        assert!(grid_pass(&bytes) > Duration::ZERO);
        let size = Size {
            rows: LINES as u16,
            cols: COLUMNS as u16,
        };
        let mut term = Terminal::new(size, Config::default());
        let mut batch = EventBatch::new();
        let mut fed = 0;
        for chunk in bytes.chunks(CHUNK) {
            fed += term.feed(chunk, &mut batch, Instant::now()).bytes;
        }
        assert_eq!(fed, bytes.len());
        assert!(term.lines_produced() > 0);
    }
    #[test]
    fn table_has_a_header_and_one_row_per_fixture() {
        let rows = vec![
            Throughput {
                fixture: "plain_ascii".into(),
                mib_per_second: 1234.56,
                ns_per_byte: 0.774,
                spread_percent: 3.2,
            },
            Throughput {
                fixture: "sixel".into(),
                mib_per_second: 1.0,
                ns_per_byte: 1.0,
                spread_percent: 0.0,
            },
        ];
        let table = render_table(&rows);
        assert!(table.starts_with("| Fixture | MiB/s | ns/B | spread |\n|---|---:|---:|---:|\n"));
        assert!(table.contains("| `plain_ascii` | 1234.6 | 0.77 | 3% |\n"));
        assert_eq!(table.lines().count(), 2 + rows.len());
    }
}
