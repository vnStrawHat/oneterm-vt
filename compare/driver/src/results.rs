//! The results JSON (output.md, `schema = 1`) and its summary arithmetic.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::corpus;
use crate::machine::{MachineRecord, Toolchains};
use crate::verify::Status;

pub const SCHEMA: u32 = 1;
pub const ONETERM: &str = "oneterm";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CorpusRecord {
    pub version: u32,
    pub mib: usize,
    pub lock_sha256: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Knobs {
    pub cols: u16,
    pub rows: u16,
    pub scrollback: usize,
    pub chunk: usize,
    pub warmup: usize,
    pub passes: usize,
    pub repetitions: usize,
    pub checkpoints: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EngineRecord {
    pub name: String,
    pub version: String,
    pub source: String,
    pub language: String,
    pub build: String,
    pub capabilities: Vec<String>,
    pub labels: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GridRow {
    pub engine: String,
    pub fixture: String,
    pub status: Status,
    pub mib_per_second: Option<f64>,
    pub ns_per_byte: Option<f64>,
    pub spread_percent: Option<f64>,
    pub ratio_to_oneterm: Option<f64>,
    pub samples_ns: Vec<u64>,
    pub steady_mib: Option<f64>,
    pub peak_mib: Option<f64>,
    pub first_difference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl GridRow {
    /// A row with no numbers: `unsupported`, or a pair that never produced any.
    pub fn bare(engine: &str, fixture: &str, status: Status) -> Self {
        Self {
            engine: engine.to_string(),
            fixture: fixture.to_string(),
            status,
            mib_per_second: None,
            ns_per_byte: None,
            spread_percent: None,
            ratio_to_oneterm: None,
            samples_ns: Vec::new(),
            steady_mib: None,
            peak_mib: None,
            first_difference: None,
            error: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusCounts {
    pub ok: u32,
    pub accepted: u32,
    pub diverged: u32,
    pub unverified: u32,
    pub unsupported: u32,
    pub failed: u32,
}

impl StatusCounts {
    fn add(&mut self, status: Status) {
        let slot = match status {
            Status::Ok => &mut self.ok,
            Status::Accepted => &mut self.accepted,
            Status::Diverged => &mut self.diverged,
            Status::Unverified => &mut self.unverified,
            Status::Unsupported => &mut self.unsupported,
            Status::Failed => &mut self.failed,
        };
        *slot += 1;
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Summary {
    pub status_counts: BTreeMap<String, StatusCounts>,
    pub intersection: Vec<String>,
    pub geomean_ratio_to_oneterm: BTreeMap<String, f64>,
    /// The median of the same ratios: one outlier fixture moves the geomean, not this.
    #[serde(default)]
    pub median_ratio_to_oneterm: BTreeMap<String, f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Results {
    pub schema: u32,
    pub date: String,
    pub unpinned: bool,
    pub machine: MachineRecord,
    pub toolchains: Toolchains,
    pub corpus: CorpusRecord,
    pub knobs: Knobs,
    pub engines: Vec<EngineRecord>,
    pub grid: Vec<GridRow>,
    pub summary: Summary,
    pub wall_time_s: u64,
}

impl Results {
    pub fn row(&self, engine: &str, fixture: &str) -> Option<&GridRow> {
        self.grid
            .iter()
            .find(|row| row.engine == engine && row.fixture == fixture)
    }

    /// Engine names in table order.
    pub fn engine_names(&self) -> Vec<&str> {
        self.engines.iter().map(|e| e.name.as_str()).collect()
    }
}

/// Sets `ratio_to_oneterm` on every verified row whose fixture has a verified oneterm row.
pub fn fill_ratios(rows: &mut [GridRow]) {
    let base: BTreeMap<String, f64> = rows
        .iter()
        .filter(|r| r.engine == ONETERM && r.status.is_verified())
        .filter_map(|r| Some((r.fixture.clone(), r.mib_per_second?)))
        .collect();
    for row in rows.iter_mut() {
        row.ratio_to_oneterm = match (
            row.status.is_verified(),
            row.mib_per_second,
            base.get(&row.fixture),
        ) {
            (true, Some(mine), Some(theirs)) if *theirs > 0.0 => Some(mine / theirs),
            _ => None,
        };
    }
}

/// The middle value of `sorted` (the mean of the two middle ones for an even count).
fn median(sorted: &[f64]) -> Option<f64> {
    let middle = sorted.len() / 2;
    let upper = *sorted.get(middle)?;
    if sorted.len() % 2 == 1 {
        return Some(upper);
    }
    Some((sorted.get(middle - 1)? + upper) / 2.0)
}

/// Status counts per engine, the intersection and the geomeans over exactly it. Engines are
/// those of `engines`; an engine that is `ok` on extra fixtures gets no extra terms.
pub fn summarize(rows: &[GridRow], engines: &[String]) -> Summary {
    let mut status_counts: BTreeMap<String, StatusCounts> = engines
        .iter()
        .map(|e| (e.clone(), StatusCounts::default()))
        .collect();
    for row in rows {
        if let Some(counts) = status_counts.get_mut(&row.engine) {
            counts.add(row.status);
        }
    }
    let verified = |engine: &str, fixture: &str| {
        rows.iter().any(|r| {
            r.engine == engine
                && r.fixture == fixture
                && r.status.is_verified()
                && r.ratio_to_oneterm.is_some()
        })
    };
    let intersection: Vec<String> = corpus::FIXTURES
        .iter()
        .filter(|f| f.class == corpus::Class::Core)
        .filter(|f| rows.iter().any(|r| r.fixture == f.name))
        .filter(|f| engines.iter().all(|e| verified(e, f.name)))
        .map(|f| f.name.to_string())
        .collect();
    let mut geomean_ratio_to_oneterm = BTreeMap::new();
    let mut median_ratio_to_oneterm = BTreeMap::new();
    if !intersection.is_empty() {
        for engine in engines {
            let mut ratios: Vec<f64> = intersection
                .iter()
                .filter_map(|fixture| {
                    rows.iter()
                        .find(|r| &r.engine == engine && &r.fixture == fixture)?
                        .ratio_to_oneterm
                })
                .collect();
            if ratios.len() != intersection.len() {
                continue;
            }
            let log_sum: f64 = ratios.iter().copied().map(f64::ln).sum();
            geomean_ratio_to_oneterm.insert(engine.clone(), (log_sum / ratios.len() as f64).exp());
            ratios.sort_by(f64::total_cmp);
            if let Some(middle) = median(&ratios) {
                median_ratio_to_oneterm.insert(engine.clone(), middle);
            }
        }
    }
    Summary {
        status_counts,
        intersection,
        geomean_ratio_to_oneterm,
        median_ratio_to_oneterm,
    }
}
