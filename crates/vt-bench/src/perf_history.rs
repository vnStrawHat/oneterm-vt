//! What a release writes under `perf/`: its own record, and the last releases measured in turns.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The platform this binary runs on, as records name it (`linux`, `windows`, `macos`).
pub(crate) const PLATFORM: &str = std::env::consts::OS;

/// `perf/history/<platform>/v<version>.json`: one run of the tiers by one build. Field order
/// is the JSON key order, which is the schema order. Its figures in MiB/s hold for the machine
/// and the minute of that run only; across versions, read [`InTurnsRecord`].
#[derive(Serialize)]
pub(crate) struct PerfRecord {
    pub(crate) engine: String,
    pub(crate) version: String,
    pub(crate) date: String,
    pub(crate) platform: String,
    pub(crate) machine: String,
    pub(crate) columns: u16,
    pub(crate) lines: u16,
    pub(crate) mib_per_fixture: u32,
    pub(crate) runs: u32,
    pub(crate) tiers: Tiers,
}

#[derive(Serialize)]
pub(crate) struct Tiers {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tier1_parser: Vec<Throughput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tier2_grid: Vec<Throughput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tier3_render: Vec<FrameCost>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tier4_resize: Vec<ResizeCost>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tier5_memory: Vec<MemoryCost>,
}

/// Tier 3: the cost of one `snapshot_update` per simulated frame, with the cells it copied.
/// `cells` counts cells copied per frame (rows copied x columns), not the viewport area.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct FrameCost {
    pub(crate) fixture: String,
    pub(crate) us_per_frame: f64,
    pub(crate) cells: u64,
}

/// Tier 4: the cost of resizing a terminal holding `scrollback_rows` rows of history, in
/// microseconds, growing from 80x24 to 100x40 and shrinking back.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ResizeCost {
    pub(crate) scrollback_rows: u32,
    pub(crate) grow_us: f64,
    pub(crate) shrink_us: f64,
}

/// Tier 5: live heap after filling the scrollback with one kind of row.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct MemoryCost {
    pub(crate) content: String,
    pub(crate) heap_bytes: u64,
    pub(crate) heap_bytes_per_row: f64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Throughput {
    pub(crate) fixture: String,
    pub(crate) mib_per_second: f64,
    pub(crate) ns_per_byte: f64,
    pub(crate) spread_percent: f64,
}

/// `perf/in-turns/<platform>/v<version>.json`: tier 2 of the last releases and of that release,
/// every build run in turns on one machine in one job. The only figures in MiB/s that compare
/// across versions, and what the chart draws.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct InTurnsRecord {
    pub(crate) engine: String,
    pub(crate) date: String,
    pub(crate) platform: String,
    pub(crate) machine: String,
    pub(crate) columns: u16,
    pub(crate) lines: u16,
    pub(crate) mib_per_fixture: u32,
    /// How often every build ran every fixture.
    pub(crate) rounds: u32,
    /// Timed cycles inside one run; a run reports their median.
    pub(crate) runs: u32,
    /// In the order they were given, oldest first; the build that measured is last.
    pub(crate) builds: Vec<BuildThroughput>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct BuildThroughput {
    pub(crate) version: String,
    /// A fixture the build lacks, or generates another stream for, has no entry.
    pub(crate) tier2_grid: Vec<BestOfRounds>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct BestOfRounds {
    pub(crate) fixture: String,
    /// The fastest round: the machine can only slow a round down.
    pub(crate) mib_per_second: f64,
    /// Every round as measured, in order: how quiet the job was.
    pub(crate) rounds: Vec<f64>,
}

/// Reads one in-turns file; the path prefixes every error.
pub(crate) fn load_in_turns(path: &Path) -> Result<InTurnsRecord> {
    let source = path.display();
    let json = fs::read_to_string(path).with_context(|| format!("{source}: cannot read file"))?;
    serde_json::from_str(&json).with_context(|| format!("{source}: invalid in-turns record"))
}

/// The UTC calendar date `YYYY-MM-DD` of a Unix timestamp (days-to-civil algorithm).
pub(crate) fn utc_date(unix_seconds: u64) -> String {
    let days = i64::try_from(unix_seconds / 86_400).unwrap_or(0) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Today's UTC date, for a record written now.
pub(crate) fn today() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    utc_date(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(tiers: Tiers) -> PerfRecord {
        PerfRecord {
            engine: "oneterm-vt".into(),
            version: "0.1.0".into(),
            date: "2026-10-23".into(),
            platform: "linux".into(),
            machine: "m".into(),
            columns: 160,
            lines: 45,
            mib_per_fixture: 1,
            runs: 5,
            tiers,
        }
    }

    fn throughput() -> Throughput {
        Throughput {
            fixture: "f".into(),
            mib_per_second: 1.0,
            ns_per_byte: 2.0,
            spread_percent: 3.0,
        }
    }

    fn assert_in_order(json: &str, keys: &[&str]) {
        let positions: Vec<usize> = keys
            .iter()
            .map(|key| json.find(&format!("\"{key}\"")).expect(key))
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{json}");
    }

    #[test]
    fn utc_date_converts_known_timestamps() {
        for (seconds, date) in [
            (0, "1970-01-01"),
            (951_782_400, "2000-02-29"),
            (1_792_713_600, "2026-10-23"),
            (4_107_542_400, "2100-03-01"),
        ] {
            assert_eq!(utc_date(seconds), date);
        }
    }

    #[test]
    fn tier1_only_record_serializes_in_schema_order() {
        let json = serde_json::to_string(&record(Tiers {
            tier1_parser: vec![throughput()],
            tier2_grid: Vec::new(),
            tier3_render: Vec::new(),
            tier4_resize: Vec::new(),
            tier5_memory: Vec::new(),
        }))
        .unwrap();
        for key in ["tier2_grid", "tier3_render", "tier4_resize", "tier5_memory"] {
            assert!(!json.contains(key), "{key}");
        }
        assert_in_order(
            &json,
            &[
                "engine",
                "version",
                "date",
                "platform",
                "machine",
                "columns",
                "lines",
                "mib_per_fixture",
                "runs",
                "tiers",
                "tier1_parser",
                "fixture",
                "mib_per_second",
                "ns_per_byte",
                "spread_percent",
            ],
        );
    }

    #[test]
    fn record_has_every_tier_in_order() {
        let json = serde_json::to_string(&record(Tiers {
            tier1_parser: vec![throughput()],
            tier2_grid: vec![throughput()],
            tier3_render: vec![FrameCost {
                fixture: "f".into(),
                us_per_frame: 2.5,
                cells: 160,
            }],
            tier4_resize: vec![ResizeCost {
                scrollback_rows: 10_000,
                grow_us: 1.5,
                shrink_us: 2.5,
            }],
            tier5_memory: vec![MemoryCost {
                content: "plain".into(),
                heap_bytes: 1000,
                heap_bytes_per_row: 0.1,
            }],
        }))
        .unwrap();
        assert_in_order(
            &json,
            &[
                "tier1_parser",
                "tier2_grid",
                "tier3_render",
                "us_per_frame",
                "cells",
                "tier4_resize",
                "scrollback_rows",
                "grow_us",
                "shrink_us",
                "tier5_memory",
                "content",
                "heap_bytes",
                "heap_bytes_per_row",
            ],
        );
    }

    #[test]
    fn in_turns_record_round_trips_in_schema_order() {
        let record = InTurnsRecord {
            engine: "oneterm-vt".into(),
            date: "2026-11-19".into(),
            platform: "linux".into(),
            machine: "m".into(),
            columns: 160,
            lines: 45,
            mib_per_fixture: 32,
            rounds: 2,
            runs: 5,
            builds: vec![BuildThroughput {
                version: "0.4.2".into(),
                tier2_grid: vec![BestOfRounds {
                    fixture: "plain_ascii".into(),
                    mib_per_second: 938.9,
                    rounds: vec![489.0, 938.9],
                }],
            }],
        };
        let json = serde_json::to_string_pretty(&record).unwrap();
        assert_in_order(
            &json,
            &[
                "engine",
                "date",
                "platform",
                "machine",
                "columns",
                "lines",
                "mib_per_fixture",
                "rounds",
                "runs",
                "builds",
                "version",
                "tier2_grid",
                "fixture",
                "mib_per_second",
            ],
        );
        let file =
            std::env::temp_dir().join(format!("vt-bench-in-turns-{}.json", std::process::id()));
        fs::write(&file, &json).unwrap();
        let loaded = load_in_turns(&file);
        fs::write(&file, "{").unwrap();
        let broken = load_in_turns(&file);
        fs::remove_file(&file).unwrap();
        assert_eq!(loaded.unwrap(), record);
        let error = format!("{:#}", broken.unwrap_err());
        assert!(error.contains("invalid in-turns record"), "{error}");
        assert!(error.contains("vt-bench-in-turns-"), "{error}");
    }
}
