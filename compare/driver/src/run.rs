//! `compare run`: verify every pair, time the supported ones, judge, and write the report files.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use compare_harness::{
    GridOutput, InfoRecord, VerifyOutput, mib_per_second, summarize as stats_of,
};

use crate::engines::{self, PROCESS_TIMEOUT, Paths, Priority};
use crate::golden;
use crate::machine::{self, MachineRecord, Powercfg};
use crate::results::{
    CorpusRecord, EngineRecord, GridRow, Knobs, Results, SCHEMA, fill_ratios, summarize,
};
use crate::verify::{self, Judgement, PairInput, Ran, Selection, Status, Timed};
use crate::{chart, corpus, report};

/// Which CPU the driver (and, by inheritance, every adapter) runs on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pin {
    Cpu(u32),
    Off,
}

/// Logical CPU 4 is a P-core thread on the agreed machine (criteria.md).
pub const DEFAULT_CPU: u32 = 4;
pub const PASSES: usize = 5;
pub const WARMUP: usize = 1;
pub const CHECKPOINTS: usize = 4;

pub struct RunOptions {
    pub machine: String,
    pub engine: Option<String>,
    pub fixture: Option<String>,
    pub repetitions: usize,
    pub keep_power_plan: bool,
    pub pin: Pin,
}

/// What the timed processes of one pair produced.
#[derive(Default)]
struct TimedPair {
    samples_ns: Vec<u64>,
    hashes: Vec<String>,
    bytes: u64,
    error: Option<String>,
}

const MIB: f64 = 1_048_576.0;

/// The grid row of one judged pair. Numbers exist for every pair that was timed, whatever
/// its status; `unsupported` and `failed` rows carry none.
fn assemble_row(
    engine: &str,
    fixture: &str,
    judgement: &Judgement,
    verified: Option<&VerifyOutput>,
    timed: Option<&TimedPair>,
) -> GridRow {
    let mut row = GridRow::bare(engine, fixture, judgement.status);
    row.first_difference.clone_from(&judgement.first_difference);
    row.error.clone_from(&judgement.error);
    if matches!(judgement.status, Status::Unsupported | Status::Failed) {
        return row;
    }
    if let Some(timed) = timed {
        if let Some(stats) = stats_of(&timed.samples_ns) {
            row.mib_per_second = Some(mib_per_second(timed.bytes, stats.median_ns));
            row.ns_per_byte = Some(stats.median_ns / timed.bytes.max(1) as f64);
            row.spread_percent = Some(stats.spread_percent);
        }
        row.samples_ns.clone_from(&timed.samples_ns);
    }
    if let Some(output) = verified {
        let memory = output.memory;
        row.steady_mib = Some(memory.steady.saturating_sub(memory.base) as f64 / MIB);
        row.peak_mib = Some(memory.peak.saturating_sub(memory.base) as f64 / MIB);
    }
    row
}

fn timed_process(paths: &Paths, engine: &str, fixture: &str) -> Result<GridOutput, String> {
    let mut command = paths.command(engine);
    command
        .arg("grid")
        .arg("--file")
        .arg(verify::corpus_file(paths, fixture))
        .args([
            "--passes",
            &PASSES.to_string(),
            "--warmup",
            &WARMUP.to_string(),
        ]);
    let text = engines::run_adapter(command, PROCESS_TIMEOUT, Priority::High)
        .map_err(|error| format!("{error:#}"))?;
    serde_json::from_str(&text).map_err(|error| format!("malformed JSON: {error}"))
}

pub fn run_command(paths: &Paths, options: &RunOptions) -> Result<PathBuf> {
    machine::validate_slug(&options.machine)?;
    let started = Instant::now();
    let selection = Selection::new(options.engine.as_deref(), options.fixture.as_deref())?;
    machine::refuse_if_building(&machine::running_processes(), std::process::id())?;
    let lock = corpus::ensure_corpus(paths)?;
    let lock_sha256 = corpus::hash_file(&corpus::lock_path(paths))?.1;

    let restore_file = paths.work_dir().join("power-restore.txt");
    let session = machine::begin_power_session(&Powercfg, &restore_file, options.keep_power_plan)?;
    if session.guard.is_some() {
        machine::install_ctrl_c_restore(&restore_file);
    }
    let pinned_cpu = match options.pin {
        Pin::Cpu(cpu) => {
            machine::pin_to_cpu(cpu)?;
            Some(cpu)
        }
        Pin::Off => None,
    };
    let on_ac = machine::on_ac_power();
    if on_ac == Some(false) {
        eprintln!("warning: running on battery");
    }

    let infos = verify::read_infos(paths, &selection.engines)?;
    let accepted = golden::read_accepted(paths)?;
    // Same word as `verify` prints: a fixture with neither golden nor ungated entry is `no-golden`
    // (its rows are `unverified` in the results, like an ungated one).
    let ungated = golden::read_ungated(paths)?;
    for fixture in &selection.fixtures {
        if golden::read_golden(paths, fixture.name)?.is_none()
            && !ungated.iter().any(|u| u.fixture == fixture.name)
        {
            eprintln!(
                "warning: {}: no-golden (not in ungated.toml either): run `compare golden`",
                fixture.name
            );
        }
    }

    // Verify pass: one process per pair, not timed.
    let mut verified: HashMap<(String, String), Ran<VerifyOutput>> = HashMap::new();
    for fixture in &selection.fixtures {
        for engine in &selection.engines {
            if !verify::supports(&infos[engine], fixture) {
                continue;
            }
            eprintln!("verify {engine} {}", fixture.name);
            let ran = verify::run_verify_process(paths, engine, fixture.name, Priority::High);
            verified.insert((engine.clone(), fixture.name.to_string()), ran);
        }
    }

    // Timed passes: repetition r, fixtures in table order, engines rotated by r.
    let mut timed: HashMap<(String, String), TimedPair> = HashMap::new();
    for repetition in 0..options.repetitions {
        for fixture in &selection.fixtures {
            for engine in engines::rotated(&selection.engines, repetition) {
                let key = (engine.clone(), fixture.name.to_string());
                if !matches!(verified.get(&key), Some(Ran::Ok(_))) {
                    continue;
                }
                let pair = timed.entry(key).or_default();
                if pair.error.is_some() {
                    continue;
                }
                eprintln!("time r{} {engine} {}", repetition + 1, fixture.name);
                match timed_process(paths, &engine, fixture.name) {
                    Ok(output) => {
                        pair.bytes = output.bytes;
                        pair.samples_ns.extend(output.samples_ns);
                        pair.hashes.push(output.final_sha256);
                    }
                    Err(error) => pair.error = Some(error),
                }
            }
        }
    }

    // Judge every pair.
    let mut grid = Vec::new();
    for fixture in &selection.fixtures {
        let golden_file = golden::read_golden(paths, fixture.name)?;
        for engine in &selection.engines {
            let key = (engine.clone(), fixture.name.to_string());
            let pair = timed.get(&key);
            let ran = verified.get(&key);
            let judgement = verify::judge(&PairInput {
                engine,
                fixture: fixture.name,
                supported: verify::supports(&infos[engine], fixture),
                verify: ran,
                golden: golden_file.as_ref(),
                timed: match pair {
                    Some(TimedPair {
                        error: Some(error), ..
                    }) => Timed::Failed(error),
                    Some(pair) => Timed::Hashes(&pair.hashes),
                    None => Timed::NotRun,
                },
                accepted: &accepted,
            });
            let output = match ran {
                Some(Ran::Ok(output)) => Some(output),
                _ => None,
            };
            grid.push(assemble_row(engine, fixture.name, &judgement, output, pair));
        }
    }
    fill_ratios(&mut grid);
    let summary = summarize(&grid, &selection.engines);

    let results = Results {
        schema: SCHEMA,
        date: machine::today(),
        unpinned: pinned_cpu.is_none(),
        machine: MachineRecord {
            name: options.machine.clone(),
            cpu: machine::cpu_brand(),
            logical_cpus: machine::logical_cpus(),
            ram_gib: machine::ram_gib(),
            os: machine::os_name(),
            power_scheme_before: session.before.label(),
            power_scheme_during: session.during.label(),
            on_ac,
            pinned_cpu,
            priority: "high".to_string(),
            power_plan_kept: options.keep_power_plan,
        },
        toolchains: machine::toolchains(&paths.root.join(".cargo-home/compare/tools/zig/zig.exe")),
        corpus: CorpusRecord {
            version: lock.version,
            mib: lock.mib,
            lock_sha256,
        },
        knobs: Knobs {
            cols: 160,
            rows: 45,
            scrollback: 10_000,
            chunk: 65_536,
            warmup: WARMUP,
            passes: PASSES,
            repetitions: options.repetitions,
            checkpoints: CHECKPOINTS,
        },
        engines: selection
            .engines
            .iter()
            .map(|e| engine_record(&infos[e]))
            .collect(),
        grid,
        summary,
        wall_time_s: started.elapsed().as_secs(),
    };
    drop(session);
    write_outputs(paths, &results)
}

fn engine_record(info: &InfoRecord) -> EngineRecord {
    EngineRecord {
        name: info.name.clone(),
        version: info.version.clone(),
        source: info.source.clone(),
        language: info.language.clone(),
        build: info.build.clone(),
        capabilities: info.capabilities.clone(),
        labels: info.labels.clone(),
    }
}

/// `perf/compare/<machine>/<date>.{json,md,svg}`; returns the JSON path.
fn write_outputs(paths: &Paths, results: &Results) -> Result<PathBuf> {
    let dir = paths.root.join("perf/compare").join(&results.machine.name);
    fs::create_dir_all(&dir)?;
    let stem = machine::output_stem(&dir, &results.date);
    let json = dir.join(format!("{stem}.json"));
    fs::write(&json, serde_json::to_string_pretty(results)? + "\n")
        .with_context(|| format!("cannot write {}", json.display()))?;
    write_report_files(&json, results)?;
    Ok(json)
}

fn write_report_files(json: &std::path::Path, results: &Results) -> Result<()> {
    fs::write(json.with_extension("md"), report::render_markdown(results))?;
    fs::write(json.with_extension("svg"), chart::render_svg(results))?;
    Ok(())
}

/// `compare report --in FILE`: regenerates the `.md` and `.svg` next to a results JSON.
pub fn report_command(input: &std::path::Path) -> Result<()> {
    let text =
        fs::read_to_string(input).with_context(|| format!("cannot read {}", input.display()))?;
    let results: Results =
        serde_json::from_str(&text).with_context(|| format!("cannot parse {}", input.display()))?;
    write_report_files(input, &results)
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod run_tests;
