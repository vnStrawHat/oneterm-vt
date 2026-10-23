//! Reading `perf/history/*.json`: one record per release.

use std::fmt;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

/// One benchmark run. Field order is the JSON key order, which is the schema order.
/// `#[serde(default)]` keeps older records readable.
#[derive(Serialize, Deserialize)]
pub(crate) struct PerfRecord {
    #[serde(default)]
    pub(crate) engine: String,
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) date: String,
    pub(crate) machine: String,
    #[serde(default)]
    pub(crate) columns: u16,
    #[serde(default)]
    pub(crate) lines: u16,
    #[serde(default)]
    pub(crate) mib_per_fixture: u32,
    #[serde(default)]
    pub(crate) runs: u32,
    pub(crate) tiers: Tiers,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Tiers {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) tier1_parser: Vec<Throughput>,
    // Still required on read: the chart needs it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tier2_grid: Vec<Throughput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) tier4_resize: Vec<ResizeCost>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) tier5_memory: Vec<MemoryCost>,
}

/// Tier 4: the cost of resizing a terminal holding `scrollback_rows` rows of history, in
/// microseconds, growing from 80x24 to 100x40 and shrinking back.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResizeCost {
    pub(crate) scrollback_rows: u32,
    pub(crate) grow_us: f64,
    pub(crate) shrink_us: f64,
}

/// Tier 5: live heap after filling the scrollback with one kind of row.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct MemoryCost {
    pub(crate) content: String,
    pub(crate) heap_bytes: u64,
    pub(crate) heap_bytes_per_row: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Throughput {
    pub(crate) fixture: String,
    pub(crate) mib_per_second: f64,
    #[serde(default)]
    pub(crate) ns_per_byte: f64,
    pub(crate) spread_percent: f64,
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

/// A release version, `N.N.N`. Field order is semver order, so the derived `Ord` sorts releases.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct ReleaseVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl FromStr for ReleaseVersion {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        let parts: Vec<&str> = text.split('.').collect();
        let [major, minor, patch] = parts.as_slice() else {
            bail!("version '{text}' is not N.N.N");
        };
        let number = |part: &str| -> Result<u64> {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                bail!("version '{text}' is not N.N.N");
            }
            part.parse()
                .map_err(|_| anyhow!("version '{text}' is not N.N.N"))
        };
        Ok(Self {
            major: number(major)?,
            minor: number(minor)?,
            patch: number(patch)?,
        })
    }
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

pub(crate) struct Release {
    pub(crate) version: ReleaseVersion,
    pub(crate) record: PerfRecord,
}

/// Parses one history file. `source` (the file path) prefixes every error.
pub(crate) fn parse_release(source: &str, json: &str) -> Result<Release> {
    let record: PerfRecord =
        serde_json::from_str(json).with_context(|| format!("{source}: invalid record"))?;
    let version = record
        .version
        .parse()
        .with_context(|| format!("{source}: bad version"))?;
    Ok(Release { version, record })
}

/// Sorts ascending by version; two records for one version are an error.
pub(crate) fn sort_releases(mut releases: Vec<Release>) -> Result<Vec<Release>> {
    releases.sort_by_key(|release| release.version);
    if let Some(pair) = releases
        .windows(2)
        .find(|pair| pair[0].version == pair[1].version)
    {
        bail!("duplicate perf record for version {}", pair[0].version);
    }
    Ok(releases)
}

/// Loads every `*.json` file directly inside `dir`, sorted by version.
pub(crate) fn load_history(dir: &Path) -> Result<Vec<Release>> {
    let entries = fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))?;
    let mut releases = Vec::new();
    for entry in entries {
        let path = entry
            .with_context(|| format!("cannot read {}", dir.display()))?
            .path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let source = path.display().to_string();
        let json =
            fs::read_to_string(&path).with_context(|| format!("{source}: cannot read file"))?;
        releases.push(parse_release(&source, &json)?);
    }
    if releases.is_empty() {
        bail!("no *.json perf records in {}", dir.display());
    }
    sort_releases(releases)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(version: &str) -> String {
        format!(
            r#"{{"version":"{version}","machine":"m","tiers":{{"tier2_grid":[{{"fixture":"f","mib_per_second":1.0,"ns_per_byte":1.0,"spread_percent":2.0}}]}}}}"#
        )
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
        let record = PerfRecord {
            engine: "oneterm-vt".into(),
            version: "0.1.0".into(),
            date: "2026-10-23".into(),
            machine: "m".into(),
            columns: 160,
            lines: 45,
            mib_per_fixture: 1,
            runs: 5,
            tiers: Tiers {
                tier1_parser: vec![Throughput {
                    fixture: "f".into(),
                    mib_per_second: 1.0,
                    ns_per_byte: 2.0,
                    spread_percent: 3.0,
                }],
                tier2_grid: Vec::new(),
                tier4_resize: Vec::new(),
                tier5_memory: Vec::new(),
            },
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(!json.contains("tier2_grid"));
        let keys = [
            "engine",
            "version",
            "date",
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
        ];
        let positions: Vec<usize> = keys
            .iter()
            .map(|key| json.find(&format!("\"{key}\"")).unwrap())
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{json}");
    }

    #[test]
    fn record_has_tiers_2_4_5() {
        let throughput = || Throughput {
            fixture: "f".into(),
            mib_per_second: 1.0,
            ns_per_byte: 2.0,
            spread_percent: 3.0,
        };
        let record = |tiers: Tiers| PerfRecord {
            engine: "oneterm-vt".into(),
            version: "0.1.0".into(),
            date: "2026-10-23".into(),
            machine: "m".into(),
            columns: 160,
            lines: 45,
            mib_per_fixture: 1,
            runs: 5,
            tiers,
        };
        let full = record(Tiers {
            tier1_parser: vec![throughput()],
            tier2_grid: vec![throughput()],
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
        });
        let json = serde_json::to_string(&full).unwrap();
        let keys = [
            "\"tier1_parser\"",
            "\"tier2_grid\"",
            "\"tier4_resize\"",
            "\"scrollback_rows\"",
            "\"grow_us\"",
            "\"shrink_us\"",
            "\"tier5_memory\"",
            "\"content\"",
            "\"heap_bytes\"",
            "\"heap_bytes_per_row\"",
        ];
        let positions: Vec<usize> = keys.iter().map(|key| json.find(key).unwrap()).collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{json}");
        let again: PerfRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(again.tiers.tier4_resize.len(), 1);
        assert_eq!(again.tiers.tier5_memory[0].content, "plain");

        let only_first = record(Tiers {
            tier1_parser: vec![throughput()],
            tier2_grid: Vec::new(),
            tier4_resize: Vec::new(),
            tier5_memory: Vec::new(),
        });
        let json = serde_json::to_string(&only_first).unwrap();
        for key in ["tier2_grid", "tier4_resize", "tier5_memory"] {
            assert!(!json.contains(key), "{key}");
        }
    }
    #[test]
    fn version_parses_three_numbers() {
        let version: ReleaseVersion = "0.1.0".parse().unwrap();
        assert_eq!(version.to_string(), "0.1.0");
    }

    #[test]
    fn version_rejects_other_shapes() {
        for text in ["0.1", "v0.1.0", "0.1.0-rc.1", "a.b.c", "1..2", "+1.2.3"] {
            assert!(text.parse::<ReleaseVersion>().is_err(), "{text}");
        }
    }

    #[test]
    fn versions_order_numerically() {
        let parse = |text: &str| text.parse::<ReleaseVersion>().unwrap();
        assert!(parse("0.9.0") < parse("0.10.0"));
        assert!(parse("0.10.0") < parse("1.0.0"));
    }

    #[test]
    fn parse_release_errors_name_the_source() {
        let invalid = parse_release("history/a.json", "{").err().unwrap();
        assert!(format!("{invalid:#}").contains("history/a.json"));
        let missing = parse_release(
            "history/b.json",
            r#"{"version":"0.1.0","machine":"m","tiers":{}}"#,
        )
        .err()
        .unwrap();
        assert!(format!("{missing:#}").contains("history/b.json"));
        let bad_version = parse_release("history/c.json", &record("0.1"))
            .err()
            .unwrap();
        assert!(format!("{bad_version:#}").contains("history/c.json"));
    }

    #[test]
    fn sort_orders_releases_and_rejects_duplicates() {
        let release = |version: &str| parse_release("t", &record(version)).unwrap();
        let sorted = sort_releases(vec![release("0.10.0"), release("0.2.0")]).unwrap();
        assert_eq!(sorted[0].version.to_string(), "0.2.0");
        assert!(sort_releases(vec![release("0.2.0"), release("0.2.0")]).is_err());
    }
}
