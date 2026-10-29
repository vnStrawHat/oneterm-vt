//! `grid --check`: tier 2 against a baseline record, a by-hand regression trip-wire.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::fixtures::{self, COLUMNS, FIXTURES, LINES};
use crate::perf_history::{self, PLATFORM, Release, Throughput};
use crate::throughput::{self, RUNS};

/// Parent of the per-platform record folders.
const HISTORY_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../perf/history");
const DEFAULT_TOLERANCE: f64 = 0.5;
const TAKES_ONLY: &str = "'grid --check' takes only --baseline, --tolerance and --mib";

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

/// What `grid --check` printed and whether every fixture held.
pub(crate) struct CheckReport {
    pub(crate) text: String,
    pub(crate) passed: bool,
}

struct CheckOptions {
    baseline: Option<PathBuf>,
    tolerance: f64,
    mib: Option<usize>,
}

/// Everything decided before the first measurement.
struct CheckPlan {
    /// The baseline path as printed.
    source: String,
    baseline: Release,
    tolerance: f64,
    mib: usize,
}

struct CheckRow {
    fixture: String,
    /// `None` for a fixture the baseline predates: shown, never judged.
    baseline: Option<f64>,
    now: f64,
    slower: bool,
}

/// Runs `grid --check`. `Err` means no verdict (exit 2).
pub(crate) fn check_command(args: &[String]) -> Result<CheckReport> {
    let history = Path::new(HISTORY_DIR).join(PLATFORM);
    let plan = prepare_check(args, BuildFlavour::current(), &history)?;
    crate::progress("tier 2 grid");
    let selected = fixtures::select(None)?;
    let now = throughput::measure_tier(plan.mib, &selected, throughput::grid_pass);
    let rows = compare(&plan.baseline.record.tiers.tier2_grid, &now, plan.tolerance);
    Ok(CheckReport {
        passed: rows.iter().all(|row| !row.slower),
        text: render_check(&plan, &rows),
    })
}

/// The refusals of check.md in their order: flags, build flavour, baseline.
fn prepare_check(args: &[String], flavour: BuildFlavour, history: &Path) -> Result<CheckPlan> {
    let options = parse_check_options(args)?;
    if let Some(reason) = skew_reason(flavour) {
        bail!("{reason}");
    }
    let (source, baseline) = match options.baseline {
        Some(path) => {
            let source = path.display().to_string();
            let json =
                fs::read_to_string(&path).with_context(|| format!("{source}: cannot read file"))?;
            let release = perf_history::parse_release(&source, &json)?;
            (source, release)
        }
        None => newest_record(history)?,
    };
    check_baseline(&source, &baseline)?;
    let recorded = baseline.record.mib_per_fixture as usize;
    let recorded_or_default = if (1..=4096).contains(&recorded) {
        recorded
    } else {
        crate::DEFAULT_MIB
    };
    Ok(CheckPlan {
        source,
        baseline,
        tolerance: options.tolerance,
        mib: options.mib.unwrap_or(recorded_or_default),
    })
}

fn parse_check_options(args: &[String]) -> Result<CheckOptions> {
    let mut options = CheckOptions {
        baseline: None,
        tolerance: DEFAULT_TOLERANCE,
        mib: None,
    };
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        if flag == "--check" {
            continue;
        }
        if !matches!(flag.as_str(), "--baseline" | "--tolerance" | "--mib") {
            bail!(TAKES_ONLY);
        }
        let Some(value) = iter.next() else {
            bail!("flag '{flag}' needs a value");
        };
        match flag.as_str() {
            "--baseline" => options.baseline = Some(PathBuf::from(value)),
            "--tolerance" => {
                options.tolerance = value
                    .parse()
                    .ok()
                    .filter(|tolerance| *tolerance > 0.0 && *tolerance <= 1.0)
                    .with_context(|| {
                        format!("--tolerance needs a number in (0, 1], got '{value}'")
                    })?;
            }
            _ => options.mib = Some(crate::parse_mib(value)?),
        }
    }
    Ok(options)
}

/// The record with the highest version in `dir`, and its name as printed.
fn newest_record(dir: &Path) -> Result<(String, Release)> {
    let newest = perf_history::load_history(dir)
        .ok()
        .and_then(|mut releases| releases.pop())
        .with_context(|| {
            format!(
                "no perf record in perf/history/{PLATFORM}; the Release workflow writes the first, or pass --baseline FILE"
            )
        })?;
    // scripts/release.sh names every record v<version>.json.
    Ok((
        format!("perf/history/{PLATFORM}/v{}.json", newest.version),
        newest,
    ))
}

/// Refusals 6a-9: platform, geometry, fixture sets, usable figures.
fn check_baseline(source: &str, baseline: &Release) -> Result<()> {
    let record = &baseline.record;
    if record.platform != PLATFORM {
        bail!(
            "{source}: baseline platform {}, this build runs on {PLATFORM}",
            record.platform
        );
    }
    let (columns, lines) = (usize::from(record.columns), usize::from(record.lines));
    if (columns, lines) != (0, 0) && (columns, lines) != (COLUMNS, LINES) {
        bail!(
            "{source}: baseline geometry {columns}x{lines}, this build measures {COLUMNS}x{LINES}"
        );
    }
    let rows = &record.tiers.tier2_grid;
    let missing: Vec<&str> = FIXTURES
        .iter()
        .map(|fixture| fixture.name)
        .filter(|name| !rows.iter().any(|row| row.fixture == *name))
        .collect();
    let extra: Vec<&str> = rows
        .iter()
        .map(|row| row.fixture.as_str())
        .filter(|name| !FIXTURES.iter().any(|fixture| fixture.name == *name))
        .collect();
    // A fixture added after the baseline was recorded is skipped by `compare`; a baseline that
    // shares no fixture with this build, or names one that is gone, is not a baseline for it.
    if missing.len() == FIXTURES.len() || !extra.is_empty() {
        bail!(
            "fixture sets differ: not in baseline: {}; not measured: {}",
            quoted_list(&missing),
            quoted_list(&extra)
        );
    }
    if let Some(row) = rows
        .iter()
        .find(|row| !(row.mib_per_second.is_finite() && row.mib_per_second > 0.0))
    {
        bail!(
            "{source}: baseline MiB/s for `{}` is not a positive number",
            row.fixture
        );
    }
    Ok(())
}

fn quoted_list(names: &[&str]) -> String {
    if names.is_empty() {
        return "-".to_owned();
    }
    let quoted: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
    quoted.join(", ")
}

/// One row per measured fixture, in `FIXTURES` order. A fixture is `slower` when `now <
/// tolerance * baseline`; exactly at the band passes. One the baseline lacks is never slower.
fn compare(baseline: &[Throughput], now: &[Throughput], tolerance: f64) -> Vec<CheckRow> {
    FIXTURES
        .iter()
        .filter_map(|fixture| {
            let after = now.iter().find(|row| row.fixture == fixture.name)?;
            let before = baseline
                .iter()
                .find(|row| row.fixture == fixture.name)
                .map(|row| row.mib_per_second);
            Some(CheckRow {
                fixture: fixture.name.to_owned(),
                baseline: before,
                now: after.mib_per_second,
                slower: before.is_some_and(|before| after.mib_per_second < tolerance * before),
            })
        })
        .collect()
}

fn render_check(plan: &CheckPlan, rows: &[CheckRow]) -> String {
    let record = &plan.baseline.record;
    let tolerance = plan.tolerance;
    let version = plan.baseline.version;
    let stamp = if record.date.is_empty() {
        format!("v{version}")
    } else {
        format!("v{version}, {}", record.date)
    };
    let mut text = format!(
        "# vt-bench grid --check\n\nBaseline: {} ({stamp})\nBaseline machine: {}\nRun: {COLUMNS}x{LINES}, {} MiB per fixture, median of {RUNS} cycles after 1 warm-up.\nA fixture fails below {tolerance:.2} x its baseline MiB/s; exactly {tolerance:.2} passes.\n\n| Fixture | baseline MiB/s | now MiB/s | ratio | verdict |\n|---|---:|---:|---:|---|\n",
        plan.source, record.machine, plan.mib
    );
    for row in rows {
        // Writing to a String cannot fail.
        let (baseline, ratio, verdict) = match row.baseline {
            Some(before) => (
                format!("{before:.1}"),
                format!("{:.2}", row.now / before),
                if row.slower { "SLOWER" } else { "ok" },
            ),
            None => ("-".to_owned(), "-".to_owned(), "no baseline"),
        };
        let _ = writeln!(
            text,
            "| `{}` | {baseline} | {:.1} | {ratio} | {verdict} |",
            row.fixture, row.now
        );
    }
    let judged = rows.iter().filter(|row| row.baseline.is_some()).count();
    let unjudged: Vec<String> = rows
        .iter()
        .filter(|row| row.baseline.is_none())
        .map(|row| format!("`{}`", row.fixture))
        .collect();
    if !unjudged.is_empty() {
        let _ = writeln!(text, "\nNot judged, no baseline: {}", unjudged.join(", "));
    }
    let slower: Vec<String> = rows
        .iter()
        .filter(|row| row.slower)
        .map(|row| format!("`{}`", row.fixture))
        .collect();
    if slower.is_empty() {
        let _ = writeln!(
            text,
            "\nPASS: {judged} of {judged} fixtures at or above {tolerance:.2} x baseline"
        );
    } else {
        let _ = writeln!(
            text,
            "\nFAIL: {} of {judged} fixtures below {tolerance:.2} x baseline: {}",
            slower.len(),
            slower.join(", ")
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use crate::perf_history::{PerfRecord, Tiers};

    use super::*;

    const CLEAN: BuildFlavour = BuildFlavour {
        debug: false,
        paranoid: false,
        profiling: false,
    };

    fn strings(parts: &[&str]) -> Vec<String> {
        parts.iter().map(ToString::to_string).collect()
    }

    fn error_text<T>(result: Result<T>) -> String {
        format!("{:#}", result.err().expect("expected an error"))
    }

    fn row(fixture: &str, mib_per_second: f64) -> Throughput {
        Throughput {
            fixture: fixture.to_owned(),
            mib_per_second,
            ns_per_byte: 0.0,
            spread_percent: 0.0,
        }
    }

    fn all_rows(mib_per_second: f64) -> Vec<Throughput> {
        FIXTURES
            .iter()
            .map(|fixture| row(fixture.name, mib_per_second))
            .collect()
    }

    fn release(
        version: &str,
        columns: u16,
        lines: u16,
        mib: u32,
        rows: Vec<Throughput>,
    ) -> Release {
        Release {
            version: version.parse().unwrap(),
            record: PerfRecord {
                engine: "oneterm-vt".to_owned(),
                version: version.to_owned(),
                date: "2026-11-30".to_owned(),
                platform: PLATFORM.to_owned(),
                machine: "box".to_owned(),
                columns,
                lines,
                mib_per_fixture: mib,
                runs: 5,
                tiers: Tiers {
                    tier1_parser: Vec::new(),
                    tier2_grid: rows,
                    tier3_render: Vec::new(),
                    tier4_resize: Vec::new(),
                    tier5_memory: Vec::new(),
                },
            },
        }
    }

    /// A scratch directory under the temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(test: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("vt-bench-check-{}-{test}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write_record(&self, name: &str, release: &Release) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, serde_json::to_string(&release.record).unwrap()).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn skew_reason_names_each_flavour() {
        let flavour = |debug, paranoid, profiling| BuildFlavour {
            debug,
            paranoid,
            profiling,
        };
        assert_eq!(skew_reason(CLEAN), None);
        assert_eq!(
            skew_reason(flavour(true, false, false)),
            Some("--check needs a release build (cargo run --release)")
        );
        assert_eq!(
            skew_reason(flavour(false, true, false)),
            Some("--check refuses under vt-paranoid: the whole-history walk dominates the timing")
        );
        assert_eq!(
            skew_reason(flavour(false, false, true)),
            Some("--check refuses under hotpath-profiling: measurement sites change the timing")
        );
        assert_eq!(
            skew_reason(flavour(true, true, true)),
            skew_reason(flavour(true, false, false))
        );
        assert_eq!(
            skew_reason(flavour(false, true, true)),
            skew_reason(flavour(false, true, false))
        );
    }

    #[test]
    fn json_refused_in_skewed_build() {
        let skewed = BuildFlavour {
            paranoid: true,
            ..CLEAN
        };
        assert_eq!(
            error_text(ensure_recordable(skewed)),
            "refusing --json: --check refuses under vt-paranoid: the whole-history walk dominates the timing"
        );
        assert!(ensure_recordable(CLEAN).is_ok());
    }

    #[test]
    fn tolerance_range() {
        let tolerance = |value: &str| parse_check_options(&strings(&["--tolerance", value]));
        for bad in ["0", "-0.5", "1.01", "x", "NaN", "inf"] {
            assert_eq!(
                error_text(tolerance(bad)),
                format!("--tolerance needs a number in (0, 1], got '{bad}'")
            );
        }
        for (good, expected) in [("0.01", 0.01), ("0.5", 0.5), ("1", 1.0)] {
            assert_eq!(tolerance(good).unwrap().tolerance, expected);
        }
        assert_eq!(parse_check_options(&[]).unwrap().tolerance, 0.5);
    }

    #[test]
    fn check_takes_only_its_flags() {
        for extra in [
            &["--json"][..],
            &["--out", "F"],
            &["--machine", "M"],
            &["--frames", "5"],
            &["--fixture", "plain_ascii"],
        ] {
            let mut args = strings(&["--check"]);
            args.extend(strings(extra));
            assert_eq!(
                error_text(parse_check_options(&args)),
                "'grid --check' takes only --baseline, --tolerance and --mib"
            );
        }
        assert!(parse_check_options(&strings(&["--check", "--mib", "0"])).is_err());
        assert_eq!(
            error_text(parse_check_options(&strings(&["--check", "--baseline"]))),
            "flag '--baseline' needs a value"
        );
    }

    #[test]
    fn exactly_at_band_passes() {
        let baseline = all_rows(100.0);
        let at_band = compare(&baseline, &all_rows(50.0), 0.5);
        assert!(at_band.iter().all(|row| !row.slower));
        let below = compare(&baseline, &all_rows(49.99), 0.5);
        assert!(below.iter().all(|row| row.slower));
    }

    #[test]
    fn rows_follow_fixture_order() {
        let mut baseline = all_rows(100.0);
        baseline.reverse();
        let rows = compare(&baseline, &all_rows(90.0), 0.5);
        let names: Vec<&str> = rows.iter().map(|row| row.fixture.as_str()).collect();
        let expected: Vec<&str> = FIXTURES.iter().map(|fixture| fixture.name).collect();
        assert_eq!(names, expected);
    }

    #[test]
    fn missing_fixture_on_either_side_refuses() {
        let mut rows: Vec<Throughput> = all_rows(1.0)
            .into_iter()
            .filter(|row| row.fixture != "sixel")
            .collect();
        rows.push(row("old_name", 1.0));
        let baseline = release("0.1.0", 160, 45, 32, rows);
        assert_eq!(
            error_text(check_baseline("b.json", &baseline)),
            "fixture sets differ: not in baseline: `sixel`; not measured: `old_name`"
        );

        let baseline = release("0.1.0", 160, 45, 32, Vec::new());
        let every = FIXTURES
            .iter()
            .map(|fixture| format!("`{}`", fixture.name))
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(
            error_text(check_baseline("b.json", &baseline)),
            format!("fixture sets differ: not in baseline: {every}; not measured: -")
        );
    }

    /// The first ten fixtures: what every record written before the newer ones holds.
    fn early_rows(mib_per_second: f64) -> Vec<Throughput> {
        let mut rows = all_rows(mib_per_second);
        rows.truncate(10);
        rows
    }

    #[test]
    fn fixtures_newer_than_the_baseline_are_skipped() {
        let baseline = release("0.1.0", 160, 45, 32, early_rows(100.0));
        check_baseline("b.json", &baseline).unwrap();

        // The new fixtures run far below any band; they still cannot fail the check.
        let rows = compare(&early_rows(100.0), &all_rows(1.0), 0.5);
        assert_eq!(rows.len(), FIXTURES.len());
        let unjudged: Vec<&str> = rows
            .iter()
            .filter(|row| row.baseline.is_none())
            .map(|row| row.fixture.as_str())
            .collect();
        assert_eq!(
            unjudged,
            [
                "grapheme_clusters",
                "hyperlinks",
                "mode_churn",
                "sixel_large"
            ]
        );
        assert!(rows.iter().all(|row| row.baseline.is_none() != row.slower));
        let plan = CheckPlan {
            source: "b.json".to_owned(),
            baseline,
            tolerance: 0.5,
            mib: 32,
        };
        let text = render_check(&plan, &rows);
        assert!(text.contains("| `hyperlinks` | - | 1.0 | - | no baseline |\n"));
        assert!(text.contains(
            "\nNot judged, no baseline: `grapheme_clusters`, `hyperlinks`, `mode_churn`, `sixel_large`\n"
        ));
        assert!(text.ends_with("\nFAIL: 10 of 10 fixtures below 0.50 x baseline: `plain_ascii`, `long_lines`, `heavy_sgr`, `tui_redraw`, `scroll_region`, `cjk_wide`, `dense_cells`, `scrolling`, `sixel`, `osc_9_7`\n"));
    }

    #[test]
    fn a_baseline_of_old_fixtures_passes_end_to_end() {
        let dir = TempDir::new("early");
        let path = dir.write_record(
            "early.json",
            &release("0.1.0", 160, 45, 32, early_rows(1.0)),
        );
        let args = strings(&["--check", "--baseline", path.to_str().unwrap()]);
        assert!(prepare_check(&args, CLEAN, &dir.0).is_ok());
    }

    #[test]
    fn the_shipped_history_still_checks() {
        let (source, newest) = newest_record(&Path::new(HISTORY_DIR).join(PLATFORM)).unwrap();
        check_baseline(&source, &newest).unwrap();
    }

    #[test]
    fn nonpositive_baseline_refuses() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut rows = all_rows(1.0);
            rows[2].mib_per_second = bad;
            let baseline = release("0.1.0", 160, 45, 32, rows);
            assert_eq!(
                error_text(check_baseline("b.json", &baseline)),
                "b.json: baseline MiB/s for `heavy_sgr` is not a positive number"
            );
        }
    }

    #[test]
    fn foreign_platform_baseline_refuses() {
        let mut baseline = release("0.1.0", 160, 45, 32, all_rows(1.0));
        baseline.record.platform = "elsewhere".to_owned();
        let expected =
            format!("b.json: baseline platform elsewhere, this build runs on {PLATFORM}");
        assert_eq!(error_text(check_baseline("b.json", &baseline)), expected);

        let dir = TempDir::new("foreign");
        let path = dir.write_record("b.json", &baseline);
        let args = strings(&["--check", "--baseline", path.to_str().unwrap()]);
        let expected = format!(
            "{}: baseline platform elsewhere, this build runs on {PLATFORM}",
            path.display()
        );
        assert_eq!(error_text(prepare_check(&args, CLEAN, &dir.0)), expected);
    }

    #[test]
    fn geometry_mismatch_refuses() {
        let baseline = release("0.1.0", 80, 24, 32, all_rows(1.0));
        assert_eq!(
            error_text(check_baseline("b.json", &baseline)),
            "b.json: baseline geometry 80x24, this build measures 160x45"
        );
        for (columns, lines) in [(0, 0), (160, 45)] {
            let baseline = release("0.1.0", columns, lines, 32, all_rows(1.0));
            assert!(check_baseline("b.json", &baseline).is_ok());
        }
    }

    #[test]
    fn newest_record_is_highest_version() {
        let dir = TempDir::new("newest");
        for version in ["0.9.0", "0.10.0", "0.2.0"] {
            dir.write_record(
                &format!("v{version}.json"),
                &release(version, 160, 45, 32, all_rows(1.0)),
            );
        }
        let (source, newest) = newest_record(&dir.0).unwrap();
        assert_eq!(source, format!("perf/history/{PLATFORM}/v0.10.0.json"));
        assert_eq!(newest.record.version, "0.10.0");
    }

    #[test]
    fn empty_or_missing_history_refuses() {
        let dir = TempDir::new("empty");
        fs::write(dir.0.join("notes.txt"), "not a record").unwrap();
        for history in [dir.0.clone(), dir.0.join("missing")] {
            let message = error_text(prepare_check(&strings(&["--check"]), CLEAN, &history));
            assert_eq!(
                message,
                format!(
                    "no perf record in perf/history/{PLATFORM}; the Release workflow writes the first, or pass --baseline FILE"
                )
            );
        }
    }

    #[test]
    fn refusals_come_in_order() {
        let dir = TempDir::new("order");
        let debug = BuildFlavour {
            debug: true,
            ..CLEAN
        };
        // A bad flag wins over the build flavour, the flavour over the baseline.
        let bad_flag = error_text(prepare_check(&strings(&["--bogus"]), debug, &dir.0));
        assert!(bad_flag.contains("takes only"), "{bad_flag}");
        let flavour = error_text(prepare_check(&[], debug, &dir.0));
        assert!(flavour.contains("release build"), "{flavour}");

        let missing = dir.0.join("nope.json");
        let args = strings(&["--baseline", missing.to_str().unwrap()]);
        let unreadable = error_text(prepare_check(&args, CLEAN, &dir.0));
        assert!(
            unreadable.contains("nope.json: cannot read file"),
            "{unreadable}"
        );

        let garbage = dir.0.join("garbage.json");
        fs::write(&garbage, "{").unwrap();
        let args = strings(&["--baseline", garbage.to_str().unwrap()]);
        let invalid = error_text(prepare_check(&args, CLEAN, &dir.0));
        assert!(
            invalid.contains("garbage.json: invalid record"),
            "{invalid}"
        );
    }

    #[test]
    fn mib_follows_the_baseline() {
        let dir = TempDir::new("mib");
        let plan = |recorded: u32, extra: &[&str]| {
            let path = dir.write_record(
                "base.json",
                &release("0.1.0", 160, 45, recorded, all_rows(1.0)),
            );
            let mut args = strings(&["--check", "--baseline", path.to_str().unwrap()]);
            args.extend(strings(extra));
            prepare_check(&args, CLEAN, &dir.0).unwrap().mib
        };
        assert_eq!(plan(8, &[]), 8);
        assert_eq!(plan(0, &[]), 32);
        assert_eq!(plan(8, &["--mib", "4"]), 4);
    }

    #[test]
    fn report_is_exact() {
        let mut baseline = release("0.1.0", 160, 45, 32, Vec::new());
        baseline.record.machine = "i7-12700 12C/20T, Windows 11 26200, rustc 1.96.0".to_owned();
        let plan = CheckPlan {
            source: "perf/history/linux/v0.1.0.json".to_owned(),
            baseline,
            tolerance: 0.5,
            mib: 32,
        };
        let check_row = |fixture: &str, baseline, now, slower| CheckRow {
            fixture: fixture.to_owned(),
            baseline,
            now,
            slower,
        };
        let rows = [
            check_row("plain_ascii", Some(82.9), 81.0, false),
            check_row("heavy_sgr", Some(204.1), 90.2, true),
        ];
        let header = "\
# vt-bench grid --check

Baseline: perf/history/linux/v0.1.0.json (v0.1.0, 2026-11-30)
Baseline machine: i7-12700 12C/20T, Windows 11 26200, rustc 1.96.0
Run: 160x45, 32 MiB per fixture, median of 5 cycles after 1 warm-up.
A fixture fails below 0.50 x its baseline MiB/s; exactly 0.50 passes.

| Fixture | baseline MiB/s | now MiB/s | ratio | verdict |
|---|---:|---:|---:|---|
";
        assert_eq!(
            render_check(&plan, &rows),
            format!(
                "{header}| `plain_ascii` | 82.9 | 81.0 | 0.98 | ok |\n| `heavy_sgr` | 204.1 | 90.2 | 0.44 | SLOWER |\n\nFAIL: 1 of 2 fixtures below 0.50 x baseline: `heavy_sgr`\n"
            )
        );
        assert_eq!(
            render_check(&plan, &rows[..1]),
            format!(
                "{header}| `plain_ascii` | 82.9 | 81.0 | 0.98 | ok |\n\nPASS: 1 of 1 fixtures at or above 0.50 x baseline\n"
            )
        );
    }

    #[test]
    fn report_without_a_date_drops_the_comma() {
        let mut baseline = release("0.1.0", 160, 45, 32, Vec::new());
        baseline.record.date.clear();
        let plan = CheckPlan {
            source: "base.json".to_owned(),
            baseline,
            tolerance: 0.25,
            mib: 8,
        };
        let text = render_check(&plan, &[]);
        assert!(text.contains("Baseline: base.json (v0.1.0)\n"));
        assert!(text.contains("fails below 0.25 x its baseline MiB/s; exactly 0.25 passes."));
    }
}
