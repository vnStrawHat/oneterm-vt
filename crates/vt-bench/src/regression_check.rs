//! `grid --check --against BINARY ...`: tier 2 of this build and of other builds of `vt-bench`,
//! every build run in turns on one machine.
//!
//! A figure in MiB/s holds only for the machine and the minute it was measured in: one hosted
//! runner description measured an unchanged engine a factor of three apart from run to run, and
//! one binary at half its speed for a whole job. So figures of two versions compare only when
//! both were measured side by side: here every build runs a fixture before any runs the next
//! one, seconds apart, and the best of several such rounds is kept. The check holds this build
//! against the last build given; the figures of all of them are the perf history of a release.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::fixtures::{COLUMNS, LINES};
use crate::perf_history::{self, BestOfRounds, BuildThroughput, InTurnsRecord};
use crate::throughput::RUNS;

/// A fixture fails below this share of the MiB/s of the build it is held against. Two builds of
/// the same engine sources, measured this way on hosted runners, read up to 2% apart on
/// Windows, 9% on Linux and 19% on macOS; a band inside that would stop releases for nothing.
const DEFAULT_TOLERANCE: f64 = 0.75;
/// Three, for the time of a release job: five read the same builds closer on macOS (13%, not
/// 19%) for two thirds more minutes.
const DEFAULT_ROUNDS: usize = 3;
const TAKES_ONLY: &str =
    "'grid --check' takes only --against, --tolerance, --mib, --rounds, --machine and --out";

/// How this binary was built. Filled with `cfg!` in one place so the rules can be tested with
/// any combination.
#[derive(Clone, Copy)]
pub(crate) struct BuildFlavour {
    pub(crate) debug: bool,
    pub(crate) paranoid: bool,
    pub(crate) profiling: bool,
}

impl BuildFlavour {
    pub(crate) fn current() -> Self {
        Self {
            debug: cfg!(debug_assertions),
            paranoid: cfg!(feature = "vt-paranoid"),
            profiling: cfg!(feature = "hotpath-profiling"),
        }
    }
}

/// Why numbers measured by this build must not be judged or recorded; `None` for a clean one.
/// Order: debug, then paranoid, then profiling.
pub(crate) fn skew_reason(flavour: BuildFlavour) -> Option<&'static str> {
    if flavour.debug {
        Some("--check needs a release build (cargo run --release)")
    } else if flavour.paranoid {
        Some("--check refuses under vt-paranoid: the whole-history walk dominates the timing")
    } else if flavour.profiling {
        Some("--check refuses under hotpath-profiling: measurement sites change the timing")
    } else {
        None
    }
}

/// `--json` on a measure command: a skewed build must never become a history record.
pub(crate) fn ensure_recordable(flavour: BuildFlavour) -> Result<()> {
    match skew_reason(flavour) {
        Some(reason) => bail!("refusing --json: {reason}"),
        None => Ok(()),
    }
}

/// What a `--check` printed and whether every fixture held.
pub(crate) struct CheckReport {
    pub(crate) text: String,
    pub(crate) passed: bool,
}

#[derive(Debug, PartialEq)]
struct CheckOptions {
    /// The other builds of `vt-bench`, in the order given; this build is held against the last.
    against: Vec<PathBuf>,
    tolerance: f64,
    mib: usize,
    rounds: usize,
    machine: Option<String>,
    /// Where the figures of every build go, as an in-turns record.
    out: Option<PathBuf>,
}

/// What one run of `grid` reported.
#[derive(Debug, Default, PartialEq)]
struct GridReport {
    /// The engine version the report names; empty when it names none.
    version: String,
    /// `(fixture, MiB/s)` in the report's order.
    rows: Vec<(String, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Verdict {
    Ok,
    Slower,
    /// The build this one is held against did not run the fixture.
    NotJudged,
}

struct CheckRow {
    fixture: String,
    /// The best round of every build, this one last; `None` where a build did not run it.
    rates: Vec<Option<f64>>,
    verdict: Verdict,
}

/// Runs `grid --check`. `Err` means no verdict (exit 2).
pub(crate) fn check_command(args: &[String]) -> Result<CheckReport> {
    let options = parse_check_options(args)?;
    if let Some(reason) = skew_reason(BuildFlavour::current()) {
        bail!("{reason}");
    }
    let mut binaries = options.against.clone();
    binaries.push(std::env::current_exe().context("cannot find this binary to run it")?);
    let fixtures: Vec<&str> = crate::fixtures::select(None)?
        .iter()
        .map(|fixture| fixture.name)
        .collect();

    let scratch = std::env::temp_dir().join(format!("vt-bench-in-turns-{}", std::process::id()));
    let shared = shared_streams(&binaries, &fixtures, &scratch);
    // Best effort: a left-over folder of streams is not a reason to fail the check.
    let _ = fs::remove_dir_all(&scratch);
    let builds = measure_in_turns(&binaries, &fixtures, &shared?, &options)?;

    let rows = check_rows(&builds, options.tolerance);
    let held_against = &binaries[binaries.len() - 2];
    if rows.iter().all(|row| row.verdict == Verdict::NotJudged) {
        bail!(
            "{}: no fixture in common with this build",
            held_against.display()
        );
    }
    let labels: Vec<String> = builds
        .iter()
        .zip(&binaries)
        .map(|(build, binary)| label(build, binary))
        .collect();
    let report = CheckReport {
        passed: rows.iter().all(|row| row.verdict != Verdict::Slower),
        text: render_check(&options, &labels, &rows),
    };
    if let Some(path) = &options.out {
        let record = InTurnsRecord {
            engine: "oneterm-vt".to_owned(),
            date: perf_history::today(),
            platform: perf_history::PLATFORM.to_owned(),
            machine: options.machine.clone().unwrap_or_default(),
            columns: COLUMNS as u16,
            lines: LINES as u16,
            mib_per_fixture: options.mib as u32,
            rounds: options.rounds as u32,
            runs: RUNS as u32,
            builds,
        };
        crate::write_file(path, &(serde_json::to_string_pretty(&record)? + "\n"))?;
    }
    Ok(report)
}

fn parse_check_options(args: &[String]) -> Result<CheckOptions> {
    let mut options = CheckOptions {
        against: Vec::new(),
        tolerance: DEFAULT_TOLERANCE,
        mib: crate::DEFAULT_MIB,
        rounds: DEFAULT_ROUNDS,
        machine: None,
        out: None,
    };
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        if flag == "--check" {
            continue;
        }
        if !matches!(
            flag.as_str(),
            "--against" | "--tolerance" | "--mib" | "--rounds" | "--machine" | "--out"
        ) {
            bail!(TAKES_ONLY);
        }
        let Some(value) = iter.next() else {
            bail!("flag '{flag}' needs a value");
        };
        match flag.as_str() {
            "--against" => options.against.push(PathBuf::from(value)),
            "--tolerance" => {
                options.tolerance = value
                    .parse()
                    .ok()
                    .filter(|tolerance| *tolerance > 0.0 && *tolerance <= 1.0)
                    .with_context(|| {
                        format!("--tolerance needs a number in (0, 1], got '{value}'")
                    })?;
            }
            "--rounds" => {
                options.rounds = value
                    .parse()
                    .ok()
                    .filter(|rounds| (1..=99).contains(rounds))
                    .with_context(|| {
                        format!("--rounds needs a whole number 1..=99, got '{value}'")
                    })?;
            }
            "--machine" => options.machine = Some(value.clone()),
            "--out" => options.out = Some(PathBuf::from(value)),
            _ => options.mib = crate::parse_mib(value)?,
        }
    }
    if options.against.is_empty() {
        bail!(
            "'grid --check' needs --against BINARY: a vt-bench built from a release to compare with"
        );
    }
    if options.out.is_some() && options.machine.is_none() {
        bail!("--out needs --machine: the record names where it was measured");
    }
    Ok(options)
}

/// For every build, which of `fixtures` it generates byte for byte as this build (the last of
/// `binaries`) does. A figure of another stream would say nothing about the engine, so a build
/// runs only the fixtures it shares. Each build writes its streams, 1 MiB each, under
/// `scratch`.
fn shared_streams(
    binaries: &[PathBuf],
    fixtures: &[&str],
    scratch: &Path,
) -> Result<Vec<Vec<bool>>> {
    let mut folders = Vec::new();
    for (index, binary) in binaries.iter().enumerate() {
        let folder = scratch.join(index.to_string());
        let output = Command::new(binary)
            .args(["fixtures", "--mib", "1", "--out"])
            .arg(&folder)
            .output()
            .with_context(|| format!("{}: cannot run it", binary.display()))?;
        if !output.status.success() {
            bail!(
                "{}: 'fixtures --mib 1' failed:\n{}",
                binary.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        folders.push(folder);
    }
    let stream = |folder: &Path, fixture: &str| fs::read(folder.join(format!("{fixture}.vt"))).ok();
    let own = &folders[folders.len() - 1];
    Ok(folders
        .iter()
        .map(|folder| {
            fixtures
                .iter()
                .map(|fixture| {
                    let ours = stream(own, fixture);
                    ours.is_some() && stream(folder, fixture) == ours
                })
                .collect()
        })
        .collect())
}

/// Every round, every fixture is run by every build that shares it before the next fixture
/// starts: the builds of one fixture are measured seconds apart, so what the machine does over
/// minutes falls on all of them alike.
fn measure_in_turns(
    binaries: &[PathBuf],
    fixtures: &[&str],
    shared: &[Vec<bool>],
    options: &CheckOptions,
) -> Result<Vec<BuildThroughput>> {
    let mut builds: Vec<BuildThroughput> = binaries
        .iter()
        .map(|_| BuildThroughput {
            version: String::new(),
            tier2_grid: Vec::new(),
        })
        .collect();
    for round in 0..options.rounds {
        crate::progress(&format!(
            "tier 2 grid in turns, round {} of {}",
            round + 1,
            options.rounds
        ));
        for (index, fixture) in fixtures.iter().enumerate() {
            for build in turn_order(binaries.len(), round) {
                if shared[build][index] {
                    let (version, rate) = run_grid(&binaries[build], fixture, options.mib)?;
                    add_round(&mut builds[build], version, fixture, rate);
                }
            }
        }
    }
    Ok(builds)
}

/// The order the builds run one fixture in: it shifts by one each round, so no build always
/// goes first or runs right after the same neighbour's heat.
fn turn_order(builds: usize, round: usize) -> impl Iterator<Item = usize> {
    (0..builds).map(move |turn| (turn + round) % builds)
}

fn add_round(build: &mut BuildThroughput, version: String, fixture: &str, rate: f64) {
    build.version = version;
    match build
        .tier2_grid
        .iter_mut()
        .find(|row| row.fixture == fixture)
    {
        Some(row) => {
            row.mib_per_second = row.mib_per_second.max(rate);
            row.rounds.push(rate);
        }
        None => build.tier2_grid.push(BestOfRounds {
            fixture: fixture.to_owned(),
            mib_per_second: rate,
            rounds: vec![rate],
        }),
    }
}

/// One `grid --fixture NAME --mib N` of `binary`, as a process of its own: every build is
/// measured the same way, each from a cold start. Returns the version it names and the MiB/s.
fn run_grid(binary: &Path, fixture: &str, mib: usize) -> Result<(String, f64)> {
    let output = Command::new(binary)
        .args(["grid", "--fixture", fixture, "--mib"])
        .arg(mib.to_string())
        .output()
        .with_context(|| format!("{}: cannot run it", binary.display()))?;
    if !output.status.success() {
        bail!(
            "{}: 'grid --fixture {fixture} --mib {mib}' failed:\n{}",
            binary.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let report = parse_grid_report(&String::from_utf8_lossy(&output.stdout));
    let rate = report
        .rows
        .iter()
        .find(|(name, _)| name == fixture)
        .map(|(_, rate)| *rate)
        .with_context(|| {
            format!(
                "{}: 'grid --fixture {fixture} --mib {mib}' printed no row for it",
                binary.display()
            )
        })?;
    Ok((report.version, rate))
}

/// Reads what `grid` prints: the version from `# vt-bench: oneterm-vt 0.4.2`, and from each
/// table row, `` | `fixture` | MiB/s | .. ``, the name and the first figure.
fn parse_grid_report(text: &str) -> GridReport {
    let mut report = GridReport::default();
    for line in text.lines() {
        if let Some(title) = line.strip_prefix("# vt-bench: ") {
            title
                .rsplit(' ')
                .next()
                .unwrap_or_default()
                .clone_into(&mut report.version);
            continue;
        }
        let mut cells = line.strip_prefix('|').unwrap_or_default().split('|');
        let fixture = cells.next().unwrap_or_default().trim();
        let rate = cells
            .next()
            .and_then(|cell| cell.trim().parse::<f64>().ok());
        let name = fixture
            .strip_prefix('`')
            .and_then(|name| name.strip_suffix('`'));
        if let (Some(name), Some(rate)) = (name, rate.filter(|rate| *rate > 0.0)) {
            report.rows.push((name.to_owned(), rate));
        }
    }
    report
}

/// How a build is named in the report: the version it reported, or its file name when it ran
/// nothing.
fn label(build: &BuildThroughput, binary: &Path) -> String {
    if build.version.is_empty() {
        binary.file_name().map_or_else(
            || binary.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    } else {
        build.version.clone()
    }
}

/// One row per fixture of this build (the last of `builds`), in its order. A fixture is slower
/// when this build is below `tolerance` x the build before it, the last one given; exactly at
/// the band passes, and a fixture that build did not run is not judged.
fn check_rows(builds: &[BuildThroughput], tolerance: f64) -> Vec<CheckRow> {
    let Some((this, others)) = builds.split_last() else {
        return Vec::new();
    };
    let rate_of = |build: &BuildThroughput, fixture: &str| {
        build
            .tier2_grid
            .iter()
            .find(|row| row.fixture == fixture)
            .map(|row| row.mib_per_second)
    };
    this.tier2_grid
        .iter()
        .map(|row| {
            let held_against = others.last().and_then(|build| rate_of(build, &row.fixture));
            let verdict = match held_against {
                None => Verdict::NotJudged,
                Some(before) if row.mib_per_second < tolerance * before => Verdict::Slower,
                Some(_) => Verdict::Ok,
            };
            CheckRow {
                fixture: row.fixture.clone(),
                rates: builds
                    .iter()
                    .map(|build| rate_of(build, &row.fixture))
                    .collect(),
                verdict,
            }
        })
        .collect()
}

/// `labels` names every build, this one last.
fn render_check(options: &CheckOptions, labels: &[String], rows: &[CheckRow]) -> String {
    let tolerance = options.tolerance;
    let others = &labels[..labels.len().saturating_sub(1)];
    let held_against = others.last().map_or("", String::as_str);
    let mut text = format!(
        "# vt-bench grid --check\n\nThis build: oneterm-vt {}\nOther builds, in the order given: {}\nRun: {COLUMNS}x{LINES}, {} MiB per fixture, {} rounds; in a round every build runs a fixture before any runs the next one. A run is the median of {RUNS} cycles after 1 warm-up; a figure is the best round, in MiB/s.\nA fixture fails below {tolerance:.2} x the figure of {held_against}, the last build given; exactly {tolerance:.2} passes.\n\n| Fixture |",
        env!("CARGO_PKG_VERSION"),
        others.join(", "),
        options.mib,
        options.rounds,
    );
    // Writing to a String cannot fail.
    for other in others {
        let _ = write!(text, " {other} |");
    }
    let _ = writeln!(text, " this build | this / {held_against} | verdict |");
    let _ = writeln!(text, "|---|{}---:|---|", "---:|".repeat(others.len() + 1));
    for row in rows {
        let _ = write!(text, "| `{}` |", row.fixture);
        for rate in &row.rates {
            match rate {
                Some(rate) => {
                    let _ = write!(text, " {rate:.1} |");
                }
                None => text.push_str(" - |"),
            }
        }
        let this = row.rates.last().copied().flatten().unwrap_or_default();
        let before = row.rates.iter().rev().nth(1).copied().flatten();
        let (ratio, verdict) = match (row.verdict, before) {
            (Verdict::Slower, Some(before)) => (format!("{:.2}", this / before), "SLOWER"),
            (Verdict::Ok, Some(before)) => (format!("{:.2}", this / before), "ok"),
            _ => ("-".to_owned(), "not judged"),
        };
        let _ = writeln!(text, " {ratio} | {verdict} |");
    }
    if rows.iter().any(|row| row.rates.contains(&None)) {
        text.push_str(
            "\n`-`: the build has no such fixture or generates another stream for it, so it did not run it.\n",
        );
    }
    let judged = rows
        .iter()
        .filter(|row| row.verdict != Verdict::NotJudged)
        .count();
    let slower: Vec<String> = rows
        .iter()
        .filter(|row| row.verdict == Verdict::Slower)
        .map(|row| format!("`{}`", row.fixture))
        .collect();
    if slower.is_empty() {
        let _ = writeln!(
            text,
            "\nPASS: {judged} of {judged} fixtures at or above {tolerance:.2} x {held_against}"
        );
    } else {
        let _ = writeln!(
            text,
            "\nFAIL: {} of {judged} fixtures below {tolerance:.2} x {held_against}: {}",
            slower.len(),
            slower.join(", ")
        );
    }
    text
}

#[cfg(test)]
#[path = "regression_check_tests.rs"]
mod regression_check_tests;
