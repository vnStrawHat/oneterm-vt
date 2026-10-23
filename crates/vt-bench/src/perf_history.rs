//! Reading `perf/history/*.json`: one record per release.

use std::fmt;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct PerfRecord {
    version: String,
    pub(crate) machine: String,
    pub(crate) tiers: Tiers,
}

#[derive(Deserialize)]
pub(crate) struct Tiers {
    pub(crate) tier2_grid: Vec<Throughput>,
}

#[derive(Deserialize)]
pub(crate) struct Throughput {
    pub(crate) fixture: String,
    pub(crate) mib_per_second: f64,
    pub(crate) spread_percent: f64,
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
