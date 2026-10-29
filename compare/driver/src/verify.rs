//! The honesty gate (verification.md): verify processes, the status of every pair, and the
//! first difference of a diverged one.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use compare_harness::{Dump, InfoRecord, VerifyOutput};
use serde::{Deserialize, Serialize};

use crate::corpus::{self, FixtureInfo};
use crate::engines::{self, PROCESS_TIMEOUT, Paths, Priority};
use crate::golden::{self, AcceptedEntry, GoldenFile};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Accepted,
    Diverged,
    Unsupported,
    Failed,
    Unverified,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Accepted => "accepted",
            Status::Diverged => "diverged",
            Status::Unsupported => "unsupported",
            Status::Failed => "failed",
            Status::Unverified => "unverified",
        }
    }

    /// Whether the pair's number counts for charts, ratios and the geomean.
    pub fn is_verified(self) -> bool {
        matches!(self, Status::Ok | Status::Accepted)
    }
}

/// What a process produced: its parsed output or the reason it failed.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Ran<T> {
    Ok(T),
    Failed(String),
}

/// The timed processes of a pair, as far as the hash tie is concerned.
#[derive(Clone, Copy, Debug)]
pub enum Timed<'a> {
    /// Not timed (the `verify` command).
    NotRun,
    /// A timed process failed.
    Failed(&'a str),
    /// The final-state hash each timed process printed.
    Hashes(&'a [String]),
}

pub struct PairInput<'a> {
    pub engine: &'a str,
    pub fixture: &'a str,
    pub supported: bool,
    pub verify: Option<&'a Ran<VerifyOutput>>,
    pub golden: Option<&'a GoldenFile>,
    pub timed: Timed<'a>,
    pub accepted: &'a [AcceptedEntry],
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Judgement {
    pub status: Status,
    pub first_difference: Option<String>,
    pub error: Option<String>,
}

impl Judgement {
    fn of(status: Status) -> Self {
        Self {
            status,
            first_difference: None,
            error: None,
        }
    }

    fn failed(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::of(Status::Failed)
        }
    }
}

/// `scrolling` overflows any scrollback; an engine that reports its history length must hold
/// nearly the 10 000 lines it was given (Ghostty prunes whole pages and holds about 9 785 after `scrolling`).
const SCROLLBACK_PROOF_FIXTURE: &str = "scrolling";
const MIN_SCROLLBACK_ROWS: u64 = 9_000;

/// The status table of verification.md, applied to one pair.
pub fn judge(input: &PairInput<'_>) -> Judgement {
    if !input.supported {
        return Judgement::of(Status::Unsupported);
    }
    let verified = match input.verify {
        None => return Judgement::failed("no verify result"),
        Some(Ran::Failed(error)) => return Judgement::failed(error.clone()),
        Some(Ran::Ok(output)) => output,
    };
    if input.fixture == SCROLLBACK_PROOF_FIXTURE
        && let Some(rows) = verified
            .scrollback_rows
            .filter(|rows| *rows < MIN_SCROLLBACK_ROWS)
    {
        return Judgement::failed(format!(
            "scrollback holds {rows} rows after `{SCROLLBACK_PROOF_FIXTURE}`, expected at least {MIN_SCROLLBACK_ROWS}"
        ));
    }
    match input.timed {
        Timed::NotRun => {}
        Timed::Failed(error) => return Judgement::failed(error),
        Timed::Hashes(hashes) => {
            if hashes.iter().any(|hash| *hash != verified.final_sha256) {
                return Judgement::failed("timed run ended in a state other than the verified one");
            }
        }
    }
    let Some(golden) = input.golden else {
        return Judgement::of(Status::Unverified);
    };
    if verified.dumps == golden.dumps {
        return Judgement::of(Status::Ok);
    }
    let accepted = input.accepted.iter().any(|entry| {
        entry.engine == input.engine
            && entry.fixture == input.fixture
            && entry.final_sha256 == verified.final_sha256
    });
    Judgement {
        status: if accepted {
            Status::Accepted
        } else {
            Status::Diverged
        },
        first_difference: Some(first_difference(&golden.dumps, &verified.dumps)),
        error: None,
    }
}

fn window(text: &str, from: usize) -> String {
    text.chars().skip(from).take(80).collect()
}

fn debug80(value: &impl std::fmt::Debug) -> String {
    format!("{value:?}").chars().take(80).collect()
}

fn first_differing_line(expected: &[String], got: &[String], field: &str) -> Option<String> {
    if let Some((row, (a, b))) = expected
        .iter()
        .zip(got)
        .enumerate()
        .find(|(_, (a, b))| a != b)
    {
        let col = a
            .chars()
            .zip(b.chars())
            .position(|(x, y)| x != y)
            .unwrap_or_else(|| a.chars().count().min(b.chars().count()));
        return Some(format!(
            "{field}[{row}] col {col}: expected {:?} got {:?}",
            window(a, col),
            window(b, col)
        ));
    }
    (expected.len() != got.len()).then(|| {
        format!(
            "{field}: expected {} rows, got {}",
            expected.len(),
            got.len()
        )
    })
}

fn first_differing_run<T: PartialEq + std::fmt::Debug>(
    expected: &[T],
    got: &[T],
    field: &str,
) -> Option<String> {
    if let Some(index) = expected.iter().zip(got).position(|(a, b)| a != b) {
        return Some(format!(
            "{field}[{index}]: expected {} got {}",
            debug80(&expected[index]),
            debug80(&got[index])
        ));
    }
    (expected.len() != got.len()).then(|| {
        format!(
            "{field}: expected {} runs, got {}{}",
            expected.len(),
            got.len(),
            match expected.get(got.len()).or(got.get(expected.len())) {
                Some(extra) => format!(", first extra {}", debug80(extra)),
                None => String::new(),
            }
        )
    })
}

/// Checkpoint, field, row and column of the first difference, expected vs got, 80 chars each.
pub fn first_difference(expected: &[Dump], got: &[Dump]) -> String {
    if expected.len() != got.len() {
        return format!(
            "checkpoints: expected {}, got {}",
            expected.len(),
            got.len()
        );
    }
    for (index, (want, have)) in expected.iter().zip(got).enumerate() {
        if want == have {
            continue;
        }
        let found = if want.cursor != have.cursor {
            Some(format!(
                "cursor: expected {:?} got {:?}",
                want.cursor, have.cursor
            ))
        } else {
            first_differing_line(&want.rows, &have.rows, "rows")
                .or_else(|| first_differing_line(&want.history, &have.history, "history"))
                .or_else(|| first_differing_run(&want.attrs, &have.attrs, "attrs"))
                .or_else(|| first_differing_run(&want.links, &have.links, "links"))
        };
        return format!("checkpoint {index}, {}", found.unwrap_or_default());
    }
    "no difference".to_string()
}

// --- processes and the verify store ---

fn store_path(paths: &Paths, engine: &str, fixture: &str) -> PathBuf {
    paths
        .verify_dir()
        .join(engine)
        .join(format!("{fixture}.json"))
}

pub fn corpus_file(paths: &Paths, fixture: &str) -> PathBuf {
    paths.corpus_dir().join(format!("{fixture}.vt"))
}

pub fn read_info(paths: &Paths, engine: &str) -> Result<InfoRecord> {
    let mut command = paths.command(engine);
    command.arg("info");
    let text = engines::run_adapter(command, PROCESS_TIMEOUT, Priority::Normal)
        .with_context(|| format!("`{engine} info` failed (is it built? run `compare build`)"))?;
    serde_json::from_str(&text).with_context(|| format!("`{engine} info` printed malformed JSON"))
}

/// One verify process; a failure becomes `Ran::Failed`, never an error of the driver.
pub fn run_verify_process(
    paths: &Paths,
    engine: &str,
    fixture: &str,
    priority: Priority,
) -> Ran<VerifyOutput> {
    let mut command = paths.command(engine);
    command
        .arg("verify")
        .arg("--file")
        .arg(corpus_file(paths, fixture));
    match engines::run_adapter(command, PROCESS_TIMEOUT, priority) {
        Err(error) => Ran::Failed(format!("{error:#}")),
        Ok(text) => match serde_json::from_str(&text) {
            Ok(output) => Ran::Ok(output),
            Err(error) => Ran::Failed(format!("malformed JSON: {error}")),
        },
    }
}

pub fn store_verify(
    paths: &Paths,
    engine: &str,
    fixture: &str,
    ran: &Ran<VerifyOutput>,
) -> Result<()> {
    let path = store_path(paths, engine, fixture);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, serde_json::to_vec(ran)?)
        .with_context(|| format!("cannot write {}", path.display()))
}

pub fn load_verify(paths: &Paths, engine: &str, fixture: &str) -> Option<Ran<VerifyOutput>> {
    let text = fs::read_to_string(store_path(paths, engine, fixture)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Whether `info` declares the capability a fixture needs.
pub fn supports(info: &InfoRecord, fixture: &FixtureInfo) -> bool {
    fixture
        .capability
        .is_none_or(|needed| info.capabilities.iter().any(|c| c == needed))
}

/// What `verify` and `run` work on.
pub struct Selection {
    pub engines: Vec<String>,
    pub fixtures: Vec<&'static FixtureInfo>,
}

impl Selection {
    pub fn new(engine: Option<&str>, fixture: Option<&str>) -> Result<Selection> {
        let engines: Vec<String> = match engine {
            Some(name) => {
                anyhow::ensure!(engines::ORDER.contains(&name), "unknown engine '{name}'");
                vec![name.to_string()]
            }
            None => engines::ORDER.iter().map(ToString::to_string).collect(),
        };
        let fixtures: Vec<&'static FixtureInfo> = match fixture {
            Some(name) => {
                vec![corpus::fixture(name).with_context(|| format!("unknown fixture '{name}'"))?]
            }
            None => corpus::FIXTURES.iter().collect(),
        };
        Ok(Selection { engines, fixtures })
    }
}

/// Runs a verify process per supported pair (unless `reuse`, which only reads the stored
/// results) and prints the status of every pair.
pub fn verify_command(paths: &Paths, selection: &Selection, reuse: bool) -> Result<()> {
    corpus::ensure_corpus(paths)?;
    let infos = read_infos(paths, &selection.engines)?;
    for fixture in selection.fixtures.iter().filter(|_| !reuse) {
        for (engine, info) in &infos {
            if !supports(info, fixture) {
                continue;
            }
            eprintln!("verify {engine} {}", fixture.name);
            let ran = run_verify_process(paths, engine, fixture.name, Priority::Normal);
            store_verify(paths, engine, fixture.name, &ran)?;
        }
    }
    print_statuses(paths, selection, &infos)
}

pub fn read_infos(paths: &Paths, engines: &[String]) -> Result<BTreeMap<String, InfoRecord>> {
    engines
        .iter()
        .map(|name| Ok((name.clone(), read_info(paths, name)?)))
        .collect()
}

/// The verify-only status of every selected pair against the committed goldens.
pub fn print_statuses(
    paths: &Paths,
    selection: &Selection,
    infos: &BTreeMap<String, InfoRecord>,
) -> Result<()> {
    let accepted = golden::read_accepted(paths)?;
    let ungated = golden::read_ungated(paths)?;
    let mut details = Vec::new();
    let mut line = format!("{:<20}", "fixture");
    for engine in &selection.engines {
        line.push_str(&format!("{engine:<12}"));
    }
    println!("{line}");
    for fixture in &selection.fixtures {
        let golden_file = golden::read_golden(paths, fixture.name)?;
        let mut line = format!("{:<20}", fixture.name);
        for engine in &selection.engines {
            let Some(info) = infos.get(engine) else {
                continue;
            };
            let ran = load_verify(paths, engine, fixture.name);
            let judgement = judge(&PairInput {
                engine,
                fixture: fixture.name,
                supported: supports(info, fixture),
                verify: ran.as_ref(),
                golden: golden_file.as_ref(),
                timed: Timed::NotRun,
                accepted: &accepted,
            });
            let mut label = judgement.status.as_str().to_string();
            if judgement.status == Status::Unverified {
                label = if ungated.iter().any(|u| u.fixture == fixture.name) {
                    "unverified".to_string()
                } else {
                    "no-golden".to_string()
                };
            }
            if let Some(difference) = &judgement.first_difference {
                details.push(format!("{engine} {}: {difference}", fixture.name));
            }
            if let Some(error) = &judgement.error {
                details.push(format!("{engine} {}: {error}", fixture.name));
            }
            line.push_str(&format!("{label:<12}"));
        }
        println!("{line}");
    }
    for detail in details {
        println!("{detail}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "verify_tests.rs"]
pub(crate) mod verify_tests;
