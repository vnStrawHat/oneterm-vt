//! `vt-bench`: benchmark tiers and the perf-history chart for `oneterm-vt`.

mod chart;
mod fixtures;
mod memory;
mod perf_history;
mod regression_check;
mod render;
mod resize;
mod throughput;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::fixtures::{COLUMNS, Fixture, LINES};
use crate::perf_history::{FrameCost, MemoryCost, PerfRecord, ResizeCost, Throughput, Tiers};
use crate::regression_check::BuildFlavour;
use crate::render::DEFAULT_FRAMES;
use crate::throughput::{CHUNK, RUNS};

// Tier 5 reads the live heap, so every allocation in this binary is counted.
#[global_allocator]
static ALLOCATOR: memory::CountingAllocator = memory::CountingAllocator;

const USAGE: &str = "\
vt-bench <command> [flags]

Measure:
  all | parser | grid | render | resize | rss
      [--mib N] [--frames N] [--json] [--out FILE] [--machine TEXT]
      [--fixture NAME]                 parser, grid, render only: one fixture, no --json
  fixtures --out DIR [--mib N]     write the ten benchmark streams as DIR/<name>.vt
  grid --check [--baseline FILE] [--tolerance R] [--mib N]
                                   compare tier 2 with the newest perf/history record

Report:
  chart --history DIR --out FILE   SVG of tier-2 MiB/s per release, one line per fixture

  help | --help | -h               print this text
";

#[cfg_attr(feature = "hotpath-profiling", hotpath::main(percentiles = [50, 99]))]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    run(&args)
}

// The binary is the one place that talks to stdout and stderr.
#[allow(clippy::print_stdout)]
fn run(args: &[String]) -> ExitCode {
    let Some(command) = args.first().map(String::as_str) else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    let outcome = match command {
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        "chart" => chart_command(&args[1..]).map(|out| println!("wrote {}", out.display())),
        "grid" if args[1..].iter().any(|arg| arg == "--check") => {
            return match regression_check::check_command(&args[1..]) {
                Ok(report) => {
                    print!("{}", report.text);
                    exit_code_for(report.passed)
                }
                // No verdict: a refusal or a flag error.
                Err(error) => {
                    eprintln!("vt-bench: {error:#}");
                    ExitCode::from(2)
                }
            };
        }
        "parser" | "grid" | "render" | "resize" | "rss" | "all" => {
            measure_command(command, &args[1..]).map(|text| print!("{text}"))
        }
        "fixtures" => fixtures_command(&args[1..]).map(|text| print!("{text}")),
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("vt-bench: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// `grid --check`: 0 when every fixture held, 1 when one is slower than the band.
fn exit_code_for(passed: bool) -> ExitCode {
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Reads `--history DIR` and `--out FILE`, writes the SVG, returns the output path.
fn chart_command(args: &[String]) -> Result<PathBuf> {
    let mut history = None;
    let mut out = None;
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let slot = match flag.as_str() {
            "--history" => &mut history,
            "--out" => &mut out,
            _ => bail!("unknown flag '{flag}' for 'chart'"),
        };
        let Some(value) = iter.next() else {
            bail!("flag '{flag}' needs a value");
        };
        *slot = Some(PathBuf::from(value));
    }
    let history = history.context("missing --history DIR")?;
    let out = out.context("missing --out FILE")?;

    let releases = perf_history::load_history(&history)?;
    write_file(&out, &chart::render_svg(&releases))?;
    Ok(out)
}

const DEFAULT_MIB: usize = 32;

struct MeasureOptions {
    mib: usize,
    json: bool,
    /// `--frames` as given; `fixtures` rejects it.
    frames: Option<usize>,
    machine: Option<String>,
    out: Option<PathBuf>,
    /// `--fixture` as given; only `parser`, `grid` and `render` take it.
    fixture: Option<String>,
}

/// `--mib N`: a whole number of MiB per fixture, `1..=4096`.
fn parse_mib(value: &str) -> Result<usize> {
    value
        .parse()
        .ok()
        .filter(|mib| (1..=4096).contains(mib))
        .with_context(|| format!("--mib needs a whole number 1..=4096, got '{value}'"))
}

/// Reads the flags shared by the measure commands.
fn parse_measure_options(command: &str, args: &[String]) -> Result<MeasureOptions> {
    let mut options = MeasureOptions {
        mib: DEFAULT_MIB,
        json: false,
        frames: None,
        machine: None,
        out: None,
        fixture: None,
    };
    let takes_fixture = matches!(command, "parser" | "grid" | "render");
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        if flag == "--json" {
            options.json = true;
            continue;
        }
        let known = match flag.as_str() {
            "--mib" | "--frames" | "--out" | "--machine" => true,
            "--fixture" => takes_fixture,
            _ => false,
        };
        if !known {
            bail!("unknown flag '{flag}' for '{command}'");
        }
        let Some(value) = iter.next() else {
            bail!("flag '{flag}' needs a value");
        };
        match flag.as_str() {
            "--mib" => options.mib = parse_mib(value)?,
            "--fixture" => options.fixture = Some(value.clone()),
            "--out" => options.out = Some(PathBuf::from(value)),
            "--machine" => options.machine = Some(value.clone()),
            "--frames" => {
                options.frames = Some(
                    value
                        .parse()
                        .ok()
                        .filter(|frames| (1..=100_000).contains(frames))
                        .with_context(|| {
                            format!("--frames needs a whole number 1..=100000, got '{value}'")
                        })?,
                );
            }
            _ => {}
        }
    }
    if options.json && options.machine.is_none() {
        bail!("--json needs --machine");
    }
    if options.json && options.fixture.is_some() {
        bail!("--fixture cannot be used with --json: a record holds every fixture");
    }
    Ok(options)
}

/// What a measure command ran: `None` for a tier it skipped.
#[derive(Default)]
struct Measured {
    tier1_parser: Option<Vec<Throughput>>,
    tier2_grid: Option<Vec<Throughput>>,
    tier3_render: Option<Vec<FrameCost>>,
    tier4_resize: Option<Vec<ResizeCost>>,
    tier5_memory: Option<Vec<MemoryCost>>,
}

/// Runs the tiers `command` names: one tier, or all five for `all`. `frames` is tier 3's
/// frame count; `fixtures` are the streams tiers 1-3 run.
fn measure(command: &str, mib: usize, frames: usize, fixtures: &[&Fixture]) -> Measured {
    let all = command == "all";
    let mut measured = Measured::default();
    if all || command == "parser" {
        progress("tier 1 parser");
        measured.tier1_parser = Some(throughput::measure_tier(
            mib,
            fixtures,
            throughput::parser_pass,
        ));
    }
    if all || command == "grid" {
        progress("tier 2 grid");
        measured.tier2_grid = Some(throughput::measure_tier(
            mib,
            fixtures,
            throughput::grid_pass,
        ));
    }
    if all || command == "render" {
        progress("tier 3 render");
        measured.tier3_render = Some(render::measure_render(frames, fixtures));
    }
    if all || command == "resize" {
        progress("tier 4 resize");
        measured.tier4_resize = Some(resize::measure_resize());
    }
    if all || command == "rss" {
        progress("tier 5 rss");
        measured.tier5_memory = Some(memory::measure_memory());
    }
    measured
}

fn progress(tier: &str) {
    eprintln!("vt-bench: measuring {tier}");
}

/// The measure commands (`parser`, `grid`, `render`, `resize`, `rss`, `all`). Returns the text for stdout.
fn measure_command(command: &str, args: &[String]) -> Result<String> {
    let options = parse_measure_options(command, args)?;
    if options.json {
        regression_check::ensure_recordable(BuildFlavour::current())?;
    }
    let fixtures = fixtures::select(options.fixture.as_deref())?;
    let measured = measure(
        command,
        options.mib,
        options.frames.unwrap_or(DEFAULT_FRAMES),
        &fixtures,
    );
    let report = if options.json {
        render_json(&options, measured)?
    } else {
        render_report(&options, &measured)
    };
    match options.out {
        Some(path) => {
            write_file(&path, &report)?;
            Ok(format!("wrote {}\n", path.display()))
        }
        None => Ok(report),
    }
}

fn render_report(options: &MeasureOptions, measured: &Measured) -> String {
    let mut report = format!(
        "# vt-bench: oneterm-vt {}\n\nGeometry {COLUMNS}x{LINES}, {} MiB per fixture, median of {RUNS} cycles after 1 warm-up, {} KiB chunks.\n",
        env!("CARGO_PKG_VERSION"),
        options.mib,
        CHUNK / 1024,
    );
    if let Some(machine) = &options.machine {
        report.push_str(&format!("Machine: {machine}\n"));
    }
    if let Some(rows) = &measured.tier1_parser {
        report.push_str("\n## Tier 1: parser alone (null Dispatch)\n\n");
        report.push_str(&throughput::render_table(rows));
    }
    if let Some(rows) = &measured.tier2_grid {
        report.push_str("\n## Tier 2: parse + grid (Terminal::feed)\n\n");
        report.push_str(&throughput::render_table(rows));
    }
    if let Some(rows) = &measured.tier3_render {
        report.push_str(&format!(
            "
## Tier 3: parse + grid + snapshot_update per frame ({} frames)

",
            options.frames.unwrap_or(DEFAULT_FRAMES)
        ));
        report.push_str(&render::render_table(rows));
    }
    if let Some(rows) = &measured.tier4_resize {
        report.push_str("\n## Tier 4: resize 80x24 <-> 100x40\n\n");
        report.push_str(&resize::render_table(rows));
    }
    if let Some(rows) = &measured.tier5_memory {
        report.push_str(&format!(
            "\n## Tier 5: live heap after {} scrollback rows\n\n",
            memory::MEMORY_ROWS
        ));
        report.push_str(&memory::render_table(rows));
    }
    report
}

fn render_json(options: &MeasureOptions, measured: Measured) -> Result<String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let record = PerfRecord {
        engine: "oneterm-vt".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        date: perf_history::utc_date(seconds),
        machine: options.machine.clone().unwrap_or_default(),
        columns: COLUMNS as u16,
        lines: LINES as u16,
        mib_per_fixture: options.mib as u32,
        runs: RUNS as u32,
        tiers: Tiers {
            tier1_parser: measured.tier1_parser.unwrap_or_default(),
            tier2_grid: measured.tier2_grid.unwrap_or_default(),
            tier3_render: measured.tier3_render.unwrap_or_default(),
            tier4_resize: measured.tier4_resize.unwrap_or_default(),
            tier5_memory: measured.tier5_memory.unwrap_or_default(),
        },
    };
    Ok(serde_json::to_string_pretty(&record)? + "\n")
}

/// `fixtures --out DIR [--mib N]`. Returns one line per written file.
fn fixtures_command(args: &[String]) -> Result<String> {
    let options = parse_measure_options("fixtures", args)?;
    if options.json || options.machine.is_some() || options.frames.is_some() {
        bail!("'fixtures' takes only --out and --mib");
    }
    let dir = options.out.context("'fixtures' needs --out DIR")?;
    let lines = fixtures::write_fixtures(&dir, options.mib)?;
    Ok(lines.iter().map(|line| format!("{line}\n")).collect())
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_lists_every_command() {
        for name in [
            "all", "parser", "grid", "render", "resize", "rss", "fixtures", "chart", "help",
            "--help", "-h",
        ] {
            assert!(USAGE.contains(name), "{name}");
        }
    }

    fn strings(parts: &[&str]) -> Vec<String> {
        parts.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn json_needs_a_machine() {
        let error = parse_measure_options("parser", &strings(&["--json"]))
            .err()
            .unwrap();
        assert!(format!("{error:#}").contains("--machine"));
        let ok = parse_measure_options("parser", &strings(&["--json", "--machine", "m"])).unwrap();
        assert!(ok.json);
        assert_eq!(ok.machine.as_deref(), Some("m"));
    }

    #[test]
    fn mib_is_ranged_and_defaults_to_32() {
        assert_eq!(parse_measure_options("parser", &[]).unwrap().mib, 32);
        for bad in ["0", "4097", "x"] {
            assert!(parse_measure_options("parser", &strings(&["--mib", bad])).is_err());
        }
        assert_eq!(
            parse_measure_options("parser", &strings(&["--mib", "4096"]))
                .unwrap()
                .mib,
            4096
        );
    }

    #[test]
    fn fixtures_needs_out() {
        let error = fixtures_command(&strings(&["--mib", "1"])).unwrap_err();
        assert!(format!("{error:#}").contains("--out"));
    }

    #[test]
    fn report_names_geometry_and_machine() {
        let options = parse_measure_options("parser", &strings(&["--machine", "box"])).unwrap();
        let measured = Measured {
            tier1_parser: Some(Vec::new()),
            ..Measured::default()
        };
        let report = render_report(&options, &measured);
        assert!(report.starts_with("# vt-bench: oneterm-vt "));
        assert!(report.contains("Geometry 160x45, 32 MiB per fixture, median of 5 cycles after 1 warm-up, 64 KiB chunks."));
        assert!(report.contains("Machine: box\n"));
        assert!(report.contains("## Tier 1: parser alone (null Dispatch)"));
        assert!(!report.contains("## Tier 2"));
    }

    #[test]
    fn report_has_a_section_per_measured_tier_in_order() {
        let options = parse_measure_options("all", &[]).unwrap();
        let measured = Measured {
            tier1_parser: Some(Vec::new()),
            tier2_grid: Some(Vec::new()),
            tier3_render: Some(Vec::new()),
            tier4_resize: Some(Vec::new()),
            tier5_memory: Some(Vec::new()),
        };
        let report = render_report(&options, &measured);
        let headers = [
            "## Tier 1: parser alone (null Dispatch)",
            "## Tier 2: parse + grid (Terminal::feed)",
            "## Tier 3: parse + grid + snapshot_update per frame (600 frames)",
            "## Tier 4: resize 80x24 <-> 100x40",
            "## Tier 5: live heap after 10000 scrollback rows",
        ];
        let positions: Vec<usize> = headers
            .iter()
            .map(|header| report.find(header).expect(header))
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn frames_flag_parses() {
        let frames = |value: &str| parse_measure_options("render", &strings(&["--frames", value]));
        for bad in ["0", "x", "100001", "-1", ""] {
            let error = frames(bad).err().expect("rejected");
            assert_eq!(
                format!("{error:#}"),
                format!("--frames needs a whole number 1..=100000, got '{bad}'")
            );
        }
        assert_eq!(frames("5").unwrap().frames, Some(5));
        assert_eq!(frames("100000").unwrap().frames, Some(100_000));
        assert_eq!(parse_measure_options("render", &[]).unwrap().frames, None);
    }

    #[test]
    fn fixtures_rejects_frames() {
        let error = fixtures_command(&strings(&["--out", "x", "--frames", "5"])).unwrap_err();
        assert!(format!("{error:#}").contains("only --out and --mib"));
    }

    #[test]
    fn usage_names_the_new_flags() {
        assert!(USAGE.contains("--tolerance R] [--mib N]"));
        assert!(USAGE.contains("--fixture NAME"));
        assert!(!USAGE.contains("lands phase by phase"));
    }

    #[test]
    fn fixture_flag_rules() {
        for command in ["parser", "grid", "render"] {
            let options =
                parse_measure_options(command, &strings(&["--fixture", "heavy_sgr"])).unwrap();
            assert_eq!(options.fixture.as_deref(), Some("heavy_sgr"));
        }
        for command in ["all", "resize", "rss", "fixtures"] {
            let error = parse_measure_options(command, &strings(&["--fixture", "heavy_sgr"]))
                .err()
                .unwrap();
            assert_eq!(
                format!("{error:#}"),
                format!("unknown flag '--fixture' for '{command}'")
            );
        }
        let error = parse_measure_options(
            "grid",
            &strings(&["--fixture", "heavy_sgr", "--json", "--machine", "m"]),
        )
        .err()
        .unwrap();
        assert_eq!(
            format!("{error:#}"),
            "--fixture cannot be used with --json: a record holds every fixture"
        );
    }

    #[test]
    fn unknown_fixture_is_refused_before_measuring() {
        let error = measure_command("grid", &strings(&["--fixture", "nope"])).unwrap_err();
        assert!(format!("{error:#}").starts_with("unknown fixture 'nope'; one of: plain_ascii"));
    }

    #[test]
    fn verdict_maps_to_exit_code() {
        assert_eq!(exit_code_for(true), ExitCode::SUCCESS);
        assert_eq!(exit_code_for(false), ExitCode::FAILURE);
        assert_eq!(
            run(&strings(&["grid", "--check", "--tolerance", "0"])),
            ExitCode::from(2)
        );
    }

    #[test]
    fn chart_requires_both_flags_and_values() {
        let args = |parts: &[&str]| parts.iter().map(ToString::to_string).collect::<Vec<_>>();
        let missing_out = chart_command(&args(&["--history", "x"])).unwrap_err();
        assert!(format!("{missing_out:#}").contains("--out"));
        let missing_history = chart_command(&args(&["--out", "x"])).unwrap_err();
        assert!(format!("{missing_history:#}").contains("--history"));
        let no_value = chart_command(&args(&["--out"])).unwrap_err();
        assert!(format!("{no_value:#}").contains("--out"));
        let unknown = chart_command(&args(&["--bogus"])).unwrap_err();
        assert!(format!("{unknown:#}").contains("--bogus"));
    }
}
