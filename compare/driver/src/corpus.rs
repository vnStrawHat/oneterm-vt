//! Corpus v1: the 23 fixtures, their classes and capabilities, and `compare/corpus.lock`.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::engines::Paths;

/// A fixture runs on every engine (`Core`) or only on engines declaring its capability.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Core,
    Capability,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Core => "core",
            Class::Capability => "capability",
        }
    }
}

pub struct FixtureInfo {
    pub name: &'static str,
    pub class: Class,
    /// The capability an engine must declare; `None` for core fixtures.
    pub capability: Option<&'static str>,
}

const fn core(name: &'static str) -> FixtureInfo {
    FixtureInfo {
        name,
        class: Class::Core,
        capability: None,
    }
}

const fn needs(name: &'static str, capability: &'static str) -> FixtureInfo {
    FixtureInfo {
        name,
        class: Class::Capability,
        capability: Some(capability),
    }
}

/// Corpus order (corpus.md): the first 14 release fixtures, then the 9 compare-only ones.
/// Release fixtures added later (`unique_styles`, `unique_links`) are not in corpus v1.
pub const FIXTURES: [FixtureInfo; 23] = [
    core("plain_ascii"),
    core("long_lines"),
    core("heavy_sgr"),
    core("tui_redraw"),
    core("scroll_region"),
    core("cjk_wide"),
    core("dense_cells"),
    core("scrolling"),
    needs("sixel", "sixel"),
    core("osc_9_7"),
    needs("grapheme_clusters", "graphemes"),
    needs("hyperlinks", "hyperlinks"),
    core("mode_churn"),
    needs("sixel_large", "sixel"),
    core("cursor_motion"),
    core("light_cells"),
    core("short_lines"),
    core("scroll_region_small"),
    core("csi_mix"),
    core("combining_unique"),
    core("long_osc"),
    core("unicode_symbols"),
    core("vim_session"),
];

pub fn fixture(name: &str) -> Option<&'static FixtureInfo> {
    FIXTURES.iter().find(|f| f.name == name)
}

pub const DEFAULT_MIB: usize = 32;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct LockEntry {
    pub name: String,
    pub class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Lock {
    pub version: u32,
    pub mib: usize,
    #[serde(rename = "fixture")]
    pub fixtures: Vec<LockEntry>,
}

pub fn lock_path(paths: &Paths) -> std::path::PathBuf {
    paths.compare_dir().join("corpus.lock")
}

pub fn read_lock(paths: &Paths) -> Result<Lock> {
    let path = lock_path(paths);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))
}

/// sha256 of a file, streamed.
pub fn hash_file(path: &Path) -> Result<(u64, String)> {
    let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut bytes = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        bytes += n as u64;
    }
    Ok((bytes, compare_harness::hex(&hasher.finalize())))
}

/// Hashes the fixture files of `dir` into lock entries, in corpus order.
pub fn hash_corpus(dir: &Path) -> Result<Vec<LockEntry>> {
    FIXTURES
        .iter()
        .map(|info| {
            let (bytes, sha256) = hash_file(&dir.join(format!("{}.vt", info.name)))?;
            Ok(LockEntry {
                name: info.name.to_string(),
                class: info.class.as_str().to_string(),
                capability: info.capability.map(str::to_string),
                bytes,
                sha256,
            })
        })
        .collect()
}

/// `Ok` when `actual` matches the lock; otherwise an error naming the first fixture that
/// differs.
pub fn check_against_lock(lock: &Lock, actual: &[LockEntry]) -> Result<()> {
    if lock.fixtures.len() != actual.len() {
        bail!(
            "corpus drift: the lock has {} fixtures, the corpus {}",
            lock.fixtures.len(),
            actual.len()
        );
    }
    for (locked, now) in lock.fixtures.iter().zip(actual) {
        if locked != now {
            bail!(
                "corpus drift in '{}': locked {} bytes sha256 {}, found {} bytes sha256 {}",
                locked.name,
                locked.bytes,
                locked.sha256,
                now.bytes,
                now.sha256
            );
        }
    }
    Ok(())
}

/// The next lock for `entries`: version 1 when there was none, unchanged when nothing
/// changed, `version + 1` otherwise.
pub fn next_lock(previous: Option<&Lock>, mib: usize, entries: Vec<LockEntry>) -> Lock {
    let version = match previous {
        None => 1,
        Some(old) if old.mib == mib && old.fixtures == entries => old.version,
        Some(old) => old.version + 1,
    };
    Lock {
        version,
        mib,
        fixtures: entries,
    }
}

/// Builds `vt-bench`, writes the 23 files into `.tmp/compare/corpus` and checks (or, with
/// `update_lock`, rewrites) `corpus.lock`.
pub fn corpus_command(paths: &Paths, mib: usize, update_lock: bool) -> Result<()> {
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "oneterm-vt-bench"])
        .current_dir(&paths.root)
        .status()
        .context("cannot run cargo")?;
    if !status.success() {
        bail!("cargo build of oneterm-vt-bench failed");
    }
    let vt_bench = paths
        .target
        .join("release")
        .join(format!("vt-bench{}", std::env::consts::EXE_SUFFIX));
    let dir = paths.corpus_dir();
    let status = Command::new(&vt_bench)
        .args(["fixtures", "--compare", "--mib", &mib.to_string(), "--out"])
        .arg(&dir)
        .stdout(std::process::Stdio::null())
        .status()
        .with_context(|| format!("cannot run {}", vt_bench.display()))?;
    if !status.success() {
        bail!("vt-bench fixtures failed");
    }
    let entries = hash_corpus(&dir)?;
    let previous = read_lock(paths).ok();
    if update_lock {
        let lock = next_lock(previous.as_ref(), mib, entries);
        let text = toml::to_string(&lock).context("cannot serialize the lock")?;
        std::fs::write(lock_path(paths), text).context("cannot write corpus.lock")?;
        eprintln!(
            "corpus.lock written: version {}, {} fixtures",
            lock.version,
            lock.fixtures.len()
        );
        return Ok(());
    }
    let lock = previous.context("compare/corpus.lock is missing: run with --update-lock")?;
    if lock.mib != mib {
        bail!("corpus.lock is for --mib {}, not {mib}", lock.mib);
    }
    check_against_lock(&lock, &entries)?;
    eprintln!(
        "corpus matches corpus.lock (version {}, {} fixtures)",
        lock.version,
        entries.len()
    );
    Ok(())
}

/// For `verify` and `run`: the corpus on disk must already match the lock.
pub fn ensure_corpus(paths: &Paths) -> Result<Lock> {
    let lock = read_lock(paths)?;
    let actual = hash_corpus(&paths.corpus_dir())
        .context("the corpus is missing or incomplete: run `compare corpus` first")?;
    check_against_lock(&lock, &actual)?;
    Ok(lock)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(tag: &str) -> Vec<LockEntry> {
        FIXTURES
            .iter()
            .map(|info| LockEntry {
                name: info.name.to_string(),
                class: info.class.as_str().to_string(),
                capability: info.capability.map(str::to_string),
                bytes: 10,
                sha256: format!("{tag}-{}", info.name),
            })
            .collect()
    }

    #[test]
    fn the_table_matches_the_corpus_spec() {
        assert_eq!(FIXTURES.len(), 23);
        let capability: Vec<(&str, &str)> = FIXTURES
            .iter()
            .filter_map(|f| f.capability.map(|c| (f.name, c)))
            .collect();
        assert_eq!(
            capability,
            [
                ("sixel", "sixel"),
                ("grapheme_clusters", "graphemes"),
                ("hyperlinks", "hyperlinks"),
                ("sixel_large", "sixel")
            ]
        );
        assert!(
            FIXTURES
                .iter()
                .all(|f| (f.class == Class::Capability) == f.capability.is_some())
        );
    }

    #[test]
    fn one_changed_byte_is_refused_naming_the_fixture() {
        let lock = next_lock(None, 32, entries("a"));
        assert_eq!(lock.version, 1);
        check_against_lock(&lock, &entries("a")).unwrap();
        let mut changed = entries("a");
        changed[6].sha256 = "dense-cells-changed".to_string();
        let error = check_against_lock(&lock, &changed).unwrap_err();
        assert!(format!("{error:#}").contains("'dense_cells'"), "{error:#}");
        assert!(check_against_lock(&lock, &changed[..22]).is_err());
    }

    #[test]
    fn the_lock_version_moves_only_when_something_changed() {
        let first = next_lock(None, 32, entries("a"));
        assert_eq!(next_lock(Some(&first), 32, entries("a")).version, 1);
        assert_eq!(next_lock(Some(&first), 32, entries("b")).version, 2);
    }

    #[test]
    fn the_lock_round_trips_through_toml() {
        let lock = next_lock(None, 32, entries("a"));
        let text = toml::to_string(&lock).unwrap();
        assert!(text.contains("[[fixture]]"));
        assert_eq!(toml::from_str::<Lock>(&text).unwrap(), lock);
    }
}
