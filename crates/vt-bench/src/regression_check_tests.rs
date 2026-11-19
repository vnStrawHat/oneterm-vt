//! `grid --check` without running a build: the flags, reading a report, the turns, the best
//! round, the band and the exact text.

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

fn rates(rows: &[(&str, f64)]) -> Vec<(String, f64)> {
    rows.iter()
        .map(|(fixture, rate)| ((*fixture).to_owned(), *rate))
        .collect()
}

/// A build whose every fixture ran one round at `rate`.
fn build(version: &str, rows: &[(&str, f64)]) -> BuildThroughput {
    let mut build = BuildThroughput {
        version: String::new(),
        tier2_grid: Vec::new(),
    };
    for (fixture, rate) in rows {
        add_round(&mut build, version.to_owned(), fixture, *rate);
    }
    build
}

fn options(against: &[&str]) -> CheckOptions {
    CheckOptions {
        against: against.iter().map(PathBuf::from).collect(),
        tolerance: 0.75,
        mib: 32,
        rounds: 3,
        machine: None,
        out: None,
    }
}

fn verdicts(rows: &[CheckRow]) -> Vec<(&str, Verdict)> {
    rows.iter()
        .map(|row| (row.fixture.as_str(), row.verdict))
        .collect()
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
fn options_have_defaults_and_take_each_flag() {
    let parsed = parse_check_options(&strings(&["--check", "--against", "old/vt-bench"])).unwrap();
    assert_eq!(parsed, options(&["old/vt-bench"]));
    let parsed = parse_check_options(&strings(&[
        "--mib",
        "4",
        "--against",
        "a",
        "--rounds",
        "3",
        "--check",
        "--tolerance",
        "0.5",
        "--against",
        "b",
        "--machine",
        "box",
        "--out",
        "turns.json",
    ]))
    .unwrap();
    assert_eq!(
        parsed,
        CheckOptions {
            // Every --against, in the order given.
            against: vec![PathBuf::from("a"), PathBuf::from("b")],
            tolerance: 0.5,
            mib: 4,
            rounds: 3,
            machine: Some("box".to_owned()),
            out: Some(PathBuf::from("turns.json")),
        }
    );
}

#[test]
fn options_are_refused_with_the_reason() {
    let refused = |args: &[&str]| error_text(parse_check_options(&strings(args)));
    assert_eq!(
        refused(&["--check"]),
        "'grid --check' needs --against BINARY: a vt-bench built from a release to compare with"
    );
    for flag in ["--baseline", "--json", "--fixture"] {
        assert_eq!(refused(&["--check", flag, "x"]), TAKES_ONLY);
    }
    assert_eq!(refused(&["--against"]), "flag '--against' needs a value");
    assert_eq!(
        refused(&["--against", "b", "--out", "turns.json"]),
        "--out needs --machine: the record names where it was measured"
    );
    for bad in ["0", "1.01", "-0.5", "half"] {
        assert_eq!(
            refused(&["--against", "b", "--tolerance", bad]),
            format!("--tolerance needs a number in (0, 1], got '{bad}'")
        );
    }
    for bad in ["0", "100", "-1", "x"] {
        assert_eq!(
            refused(&["--against", "b", "--rounds", bad]),
            format!("--rounds needs a whole number 1..=99, got '{bad}'")
        );
    }
    assert_eq!(
        refused(&["--against", "b", "--mib", "0"]),
        "--mib needs a whole number 1..=4096, got '0'"
    );
}

#[test]
fn a_debug_build_gives_no_verdict() {
    // The test profile is a debug build: the refusal comes before anything is run.
    let error = error_text(check_command(&strings(&[
        "--check",
        "--against",
        "no-such-binary",
    ])));
    assert_eq!(error, "--check needs a release build (cargo run --release)");
}

#[test]
fn a_binary_that_cannot_run_is_named() {
    let missing = Path::new("no-such-dir/no-such-vt-bench");
    let error = error_text(run_grid(missing, "plain_ascii", 1));
    assert!(
        error.starts_with("no-such-dir/no-such-vt-bench: cannot run it"),
        "{error}"
    );
    let scratch = std::env::temp_dir().join(format!("vt-bench-no-streams-{}", std::process::id()));
    let error = error_text(shared_streams(
        &[missing.to_path_buf()],
        &["plain_ascii"],
        &scratch,
    ));
    assert!(
        error.starts_with("no-such-dir/no-such-vt-bench: cannot run it"),
        "{error}"
    );
}

#[test]
fn a_grid_report_gives_the_version_and_one_rate_per_fixture() {
    let text = "\
# vt-bench: oneterm-vt 0.4.2

Geometry 160x45, 32 MiB per fixture, median of 5 cycles after 1 warm-up, 64 KiB chunks.

## Tier 2: parse + grid (Terminal::feed)

| Fixture | MiB/s | ns/B | spread |
|---|---:|---:|---:|
| `plain_ascii` | 926.6 | 1.03 | 4% |
| `kitty_images` | 538.3 | 1.77 | 12% |
";
    assert_eq!(
        parse_grid_report(text),
        GridReport {
            version: "0.4.2".to_owned(),
            rows: rates(&[("plain_ascii", 926.6), ("kitty_images", 538.3)]),
        }
    );
    // What this build prints is what it reads: a change of one without the other fails here.
    let own = crate::throughput::render_table(&[crate::perf_history::Throughput {
        fixture: "heavy_sgr".to_owned(),
        mib_per_second: 270.14,
        ns_per_byte: 3.5,
        spread_percent: 2.0,
    }]);
    assert_eq!(parse_grid_report(&own).rows, rates(&[("heavy_sgr", 270.1)]));
    // No table, no rows; a row without a positive rate is not a measurement.
    assert_eq!(
        parse_grid_report("vt-bench: unknown command\n"),
        GridReport::default()
    );
    assert!(
        parse_grid_report("| `a` | 0.0 | 1 |\n| `b` | fast | 1 |\n| c | 5.0 |\n")
            .rows
            .is_empty()
    );
}

#[test]
fn the_turn_order_shifts_by_one_each_round() {
    let order = |round| turn_order(3, round).collect::<Vec<_>>();
    assert_eq!(order(0), [0, 1, 2]);
    assert_eq!(order(1), [1, 2, 0]);
    assert_eq!(order(2), [2, 0, 1]);
    assert_eq!(order(3), [0, 1, 2]);
}

#[test]
fn the_best_round_is_kept_beside_every_round() {
    let mut build = build("0.4.1", &[("a", 100.0), ("b", 30.0)]);
    add_round(&mut build, "0.4.2".to_owned(), "a", 58.0);
    add_round(&mut build, "0.4.2".to_owned(), "b", 31.5);
    add_round(&mut build, "0.4.2".to_owned(), "a", 101.0);
    assert_eq!(build.version, "0.4.2");
    let rows: Vec<(&str, f64, &[f64])> = build
        .tier2_grid
        .iter()
        .map(|row| (row.fixture.as_str(), row.mib_per_second, &row.rounds[..]))
        .collect();
    assert_eq!(
        rows,
        [
            ("a", 101.0, &[100.0, 58.0, 101.0][..]),
            ("b", 31.5, &[30.0, 31.5][..])
        ]
    );
}

#[test]
fn a_build_is_named_by_its_version_or_its_file() {
    let path = Path::new(".tmp/previous/vt-bench-v0.4.2");
    assert_eq!(label(&build("0.4.2", &[("a", 1.0)]), path), "0.4.2");
    // It shared no fixture, so it ran nothing and reported no version.
    assert_eq!(label(&build("", &[]), path), "vt-bench-v0.4.2");
}

#[test]
fn this_build_is_held_against_the_last_one_given() {
    let builds = [
        // An older release far ahead does not fail this build, and one far behind does not
        // excuse it: only the last one given is the measure.
        build("0.3.0", &[("a", 500.0), ("b", 10.0), ("c", 100.0)]),
        build(
            "0.4.2",
            &[("a", 100.0), ("b", 100.0), ("c", 100.0), ("gone", 50.0)],
        ),
        build(
            "0.4.3",
            &[("a", 75.0), ("b", 74.9), ("c", 240.0), ("new", 1.0)],
        ),
    ];
    let rows = check_rows(&builds, 0.75);
    // In this build's order; exactly at the band passes; a fixture the last build given did
    // not run is shown and not judged, one only another build has is not a row.
    assert_eq!(
        verdicts(&rows),
        [
            ("a", Verdict::Ok),
            ("b", Verdict::Slower),
            ("c", Verdict::Ok),
            ("new", Verdict::NotJudged)
        ]
    );
    assert_eq!(rows[0].rates, [Some(500.0), Some(100.0), Some(75.0)]);
    assert_eq!(rows[3].rates, [None, None, Some(1.0)]);
    assert!(check_rows(&[], 0.75).is_empty());
}

#[test]
fn a_slowdown_of_every_fixture_fails() {
    // What a stored record could not tell from a slower runner: the builds ran in turns on
    // this one, so a build at 0.7 of the other is slower.
    let builds = [
        build("0.4.2", &[("a", 100.0), ("b", 300.0), ("c", 50.0)]),
        build("0.4.3", &[("a", 70.0), ("b", 210.0), ("c", 35.0)]),
    ];
    let rows = check_rows(&builds, 0.75);
    assert!(rows.iter().all(|row| row.verdict == Verdict::Slower));
}

#[test]
fn report_is_exact() {
    let builds = [
        build("0.4.1", &[("plain_ascii", 880.0), ("heavy_sgr", 310.0)]),
        build("0.4.2", &[("plain_ascii", 900.0), ("heavy_sgr", 300.0)]),
        build(
            "0.4.3",
            &[
                ("plain_ascii", 910.0),
                ("heavy_sgr", 210.0),
                ("brand_new", 77.0),
            ],
        ),
    ];
    let labels = strings(&["0.4.1", "0.4.2", "0.4.3"]);
    let rows = check_rows(&builds, 0.75);
    let header = format!(
        "\
# vt-bench grid --check

This build: oneterm-vt {}
Other builds, in the order given: 0.4.1, 0.4.2
Run: 160x45, 32 MiB per fixture, 3 rounds; in a round every build runs a fixture before any runs the next one. A run is the median of 5 cycles after 1 warm-up; a figure is the best round, in MiB/s.
A fixture fails below 0.75 x the figure of 0.4.2, the last build given; exactly 0.75 passes.

| Fixture | 0.4.1 | 0.4.2 | this build | this / 0.4.2 | verdict |
|---|---:|---:|---:|---:|---|
",
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(
        render_check(&options(&["a", "b"]), &labels, &rows),
        format!(
            "{header}| `plain_ascii` | 880.0 | 900.0 | 910.0 | 1.01 | ok |\n| `heavy_sgr` | 310.0 | 300.0 | 210.0 | 0.70 | SLOWER |\n| `brand_new` | - | - | 77.0 | - | not judged |\n\n`-`: the build has no such fixture or generates another stream for it, so it did not run it.\n\nFAIL: 1 of 2 fixtures below 0.75 x 0.4.2: `heavy_sgr`\n"
        )
    );
    // Every build ran every fixture, and all held: no note, a pass.
    let text = render_check(&options(&["a", "b"]), &labels, &rows[..1]);
    assert!(
        text.ends_with("| 1.01 | ok |\n\nPASS: 1 of 1 fixtures at or above 0.75 x 0.4.2\n"),
        "{text}"
    );
}
