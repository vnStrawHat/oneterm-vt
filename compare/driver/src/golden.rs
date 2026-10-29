//! Goldens by plurality (verification.md): the dump set shared by the most engines, if at
//! least three share it; otherwise no golden and the fixture is listed in `ungated.toml`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use compare_harness::{Dump, InfoRecord, VerifyOutput, sha256_hex};
use serde::{Deserialize, Serialize};

use crate::corpus;
use crate::engines::Paths;
use crate::verify::{self, Ran, Selection};

/// Engines that must agree before a dump set becomes the golden.
pub const MIN_AGREEING: usize = 3;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct GoldenFile {
    pub fixture: String,
    pub corpus_version: u32,
    pub fixture_sha256: String,
    pub agreeing_engines: Vec<String>,
    pub dumps: Vec<Dump>,
}

/// An owner-accepted divergence (`compare/accepted.toml`; agents never add entries).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct AcceptedEntry {
    pub engine: String,
    pub fixture: String,
    pub final_sha256: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct UngatedEntry {
    pub fixture: String,
    pub corpus_sha256: String,
    /// Sizes of the agreement groups, largest first.
    pub group_sizes: Vec<usize>,
}

#[derive(Serialize, Deserialize, Default)]
struct AcceptedFile {
    #[serde(default)]
    accepted: Vec<AcceptedEntry>,
}

#[derive(Serialize, Deserialize, Default)]
struct UngatedFile {
    #[serde(default)]
    ungated: Vec<UngatedEntry>,
}

pub fn golden_dir(paths: &Paths) -> PathBuf {
    paths.compare_dir().join("golden")
}

fn golden_path(paths: &Paths, fixture: &str) -> PathBuf {
    golden_dir(paths).join(format!("{fixture}.json"))
}

pub fn read_golden(paths: &Paths, fixture: &str) -> Result<Option<GoldenFile>> {
    let path = golden_path(paths, fixture);
    if !path.exists() {
        return Ok(None);
    }
    let text =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text)
        .map(Some)
        .with_context(|| format!("cannot parse {}", path.display()))
}

pub fn read_accepted(paths: &Paths) -> Result<Vec<AcceptedEntry>> {
    let path = paths.compare_dir().join("accepted.toml");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)?;
    let file: AcceptedFile =
        toml::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))?;
    Ok(file.accepted)
}

pub fn read_ungated(paths: &Paths) -> Result<Vec<UngatedEntry>> {
    let path = golden_dir(paths).join("ungated.toml");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)?;
    let file: UngatedFile =
        toml::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))?;
    Ok(file.ungated)
}

/// What the plurality rule decided for one fixture.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// This dump set, shared by these engines.
    Golden {
        dumps: Vec<Dump>,
        agreeing: Vec<String>,
    },
    /// No set is shared by three engines, or two sets tie for the most: group sizes, largest
    /// first.
    Ungated { group_sizes: Vec<usize> },
}

/// Groups the engines by their dump sets. The largest group wins when it has at least
/// [`MIN_AGREEING`] members and no other group is as large.
pub fn decide(votes: &[(String, &VerifyOutput)]) -> Decision {
    let mut groups: BTreeMap<String, (Vec<String>, &[Dump])> = BTreeMap::new();
    for (engine, output) in votes {
        let key = sha256_hex(
            serde_json::to_string(&output.dumps)
                .unwrap_or_default()
                .as_bytes(),
        );
        let group = groups
            .entry(key)
            .or_insert_with(|| (Vec::new(), &output.dumps));
        group.0.push(engine.clone());
    }
    let mut ranked: Vec<(Vec<String>, &[Dump])> = groups.into_values().collect();
    ranked.sort_by_key(|(engines, _)| std::cmp::Reverse(engines.len()));
    let group_sizes: Vec<usize> = ranked.iter().map(|(engines, _)| engines.len()).collect();
    match ranked.first() {
        Some((engines, dumps))
            if engines.len() >= MIN_AGREEING
                && ranked
                    .get(1)
                    .is_none_or(|next| next.0.len() < engines.len()) =>
        {
            Decision::Golden {
                dumps: dumps.to_vec(),
                agreeing: engines.clone(),
            }
        }
        _ => Decision::Ungated { group_sizes },
    }
}

fn write_golden(paths: &Paths, golden: &GoldenFile) -> Result<()> {
    // One dump per line, so a change reviews as a line diff.
    let mut text = format!(
        "{{\"fixture\":{},\"corpus_version\":{},\"fixture_sha256\":{},\"agreeing_engines\":{},\"dumps\":[\n",
        serde_json::to_string(&golden.fixture)?,
        golden.corpus_version,
        serde_json::to_string(&golden.fixture_sha256)?,
        serde_json::to_string(&golden.agreeing_engines)?,
    );
    let dumps: Vec<String> = golden.dumps.iter().map(Dump::canonical_json).collect();
    text.push_str(&dumps.join(",\n"));
    text.push_str("\n]}\n");
    fs::create_dir_all(golden_dir(paths))?;
    fs::write(golden_path(paths, &golden.fixture), text)?;
    Ok(())
}

fn write_ungated(paths: &Paths, entries: &[UngatedEntry]) -> Result<()> {
    let mut text = String::from(
        "# Fixtures without a 3-engine consensus: no golden, every engine `unverified` there\n\
         # (docs/specs/0016-cross-engine-bench/verification.md). Written by `compare golden`.\n\n",
    );
    text.push_str(&toml::to_string(&UngatedFile {
        ungated: entries.to_vec(),
    })?);
    fs::write(golden_dir(paths).join("ungated.toml"), text)?;
    Ok(())
}

/// `compare golden [--fixture F] [--replace]`.
pub fn golden_command(paths: &Paths, fixture: Option<&str>, replace: bool) -> Result<()> {
    let lock = corpus::ensure_corpus(paths)?;
    let selection = Selection::new(None, fixture)?;
    let infos: BTreeMap<String, InfoRecord> = verify::read_infos(paths, &selection.engines)?;
    let mut ungated = read_ungated(paths)?;
    for info in &selection.fixtures {
        let name = info.name;
        if !replace && golden_path(paths, name).exists() {
            println!("{name}: golden kept (use --replace)");
            continue;
        }
        let ran: Vec<(String, Ran<VerifyOutput>)> = selection
            .engines
            .iter()
            .filter(|engine| {
                infos
                    .get(*engine)
                    .is_some_and(|i| verify::supports(i, info))
            })
            .filter_map(|engine| Some((engine.clone(), verify::load_verify(paths, engine, name)?)))
            .collect();
        let votes: Vec<(String, &VerifyOutput)> = ran
            .iter()
            .filter_map(|(engine, ran)| match ran {
                Ran::Ok(output) => Some((engine.clone(), output)),
                Ran::Failed(_) => None,
            })
            .collect();
        let entry = lock
            .fixtures
            .iter()
            .find(|e| e.name == name)
            .context("fixture missing from corpus.lock")?;
        ungated.retain(|u| u.fixture != name);
        match decide(&votes) {
            Decision::Golden { dumps, agreeing } => {
                println!(
                    "{name}: golden from {} engines: {}",
                    agreeing.len(),
                    agreeing.join(", ")
                );
                write_golden(
                    paths,
                    &GoldenFile {
                        fixture: name.to_string(),
                        corpus_version: lock.version,
                        fixture_sha256: entry.sha256.clone(),
                        agreeing_engines: agreeing,
                        dumps,
                    },
                )?;
            }
            Decision::Ungated { group_sizes } => {
                println!("{name}: NO golden, groups {group_sizes:?} (ungated)");
                let path = golden_path(paths, name);
                if path.exists() {
                    fs::remove_file(path)?;
                }
                ungated.push(UngatedEntry {
                    fixture: name.to_string(),
                    corpus_sha256: entry.sha256.clone(),
                    group_sizes,
                });
            }
        }
    }
    let order = |fixture: &str| corpus::FIXTURES.iter().position(|f| f.name == fixture);
    ungated.sort_by_key(|u| order(&u.fixture));
    write_ungated(paths, &ungated)
}

#[cfg(test)]
#[path = "golden_tests.rs"]
mod golden_tests;
