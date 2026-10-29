//! The timed `grid` pass and the statistics both sides use.

use std::hint::black_box;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::dump::DumpBuilder;
use crate::engine::{Engine, Geometry};

/// `grid` output: the bytes fed, one duration per timed pass, and the hash of the last
/// pass's final state (computed after the timer).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GridOutput {
    pub bytes: u64,
    pub samples_ns: Vec<u64>,
    pub final_sha256: String,
}

/// Runs `warmup` untimed and `passes` timed passes. Each pass builds a fresh engine before
/// the timer and drops it after; the timer stops after the last `feed` and the `finish`.
pub fn run_grid<E: Engine>(
    bytes: &[u8],
    geometry: Geometry,
    chunk: usize,
    passes: usize,
    warmup: usize,
) -> GridOutput {
    let chunk = chunk.max(1);
    let mut samples_ns = Vec::with_capacity(passes);
    let mut final_sha256 = String::new();
    let total = warmup + passes;
    for pass in 0..total {
        let mut engine = E::new(geometry);
        let start = Instant::now();
        for part in bytes.chunks(chunk) {
            engine.feed(black_box(part));
        }
        engine.finish();
        let elapsed = start.elapsed();
        if pass >= warmup {
            samples_ns.push(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX));
        }
        if pass + 1 == total {
            let mut builder = DumpBuilder::new(geometry.cols, true);
            engine.dump(&mut builder);
            final_sha256 = builder.finish().sha256();
        }
        drop(black_box(engine));
    }
    GridOutput {
        bytes: bytes.len() as u64,
        samples_ns,
        final_sha256,
    }
}

/// Median and spread of a set of pass durations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub median_ns: f64,
    pub p10_ns: f64,
    pub p90_ns: f64,
    /// `(p90 - p10) / median x 100`.
    pub spread_percent: f64,
}

fn percentile(sorted: &[u64], fraction: f64) -> f64 {
    let position = fraction * (sorted.len() - 1) as f64;
    sorted[position.round() as usize] as f64
}

/// `None` for an empty set. A zero median gives a zero spread rather than infinity.
pub fn summarize(samples: &[u64]) -> Option<Stats> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    let median_ns = if sorted.len() % 2 == 1 {
        sorted[middle] as f64
    } else {
        (sorted[middle - 1] + sorted[middle]) as f64 / 2.0
    };
    let (p10_ns, p90_ns) = (percentile(&sorted, 0.10), percentile(&sorted, 0.90));
    let spread_percent = if median_ns > 0.0 {
        (p90_ns - p10_ns) / median_ns * 100.0
    } else {
        0.0
    };
    Some(Stats {
        median_ns,
        p10_ns,
        p90_ns,
        spread_percent,
    })
}

/// MiB (2^20 bytes) per second for `bytes` in `median_ns`; a zero duration counts as 1 ns.
pub fn mib_per_second(bytes: u64, median_ns: f64) -> f64 {
    bytes as f64 / 1_048_576.0 / (median_ns.max(1.0) / 1e9)
}

#[cfg(test)]
#[path = "timing_tests.rs"]
mod timing_tests;
