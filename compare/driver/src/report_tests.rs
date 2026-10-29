use crate::machine::{MachineRecord, Toolchains};
use crate::results::{
    CorpusRecord, EngineRecord, GridRow, Knobs, Results, SCHEMA, fill_ratios, summarize,
};

use super::*;

fn engine(name: &str, labels: &[&str]) -> EngineRecord {
    EngineRecord {
        name: name.into(),
        version: "1.0".into(),
        source: "test".into(),
        language: "rust".into(),
        build: "release".into(),
        capabilities: vec!["hyperlinks".into()],
        labels: labels.iter().map(ToString::to_string).collect(),
    }
}

pub(crate) fn row(engine: &str, fixture: &str, status: Status, rate: f64, spread: f64) -> GridRow {
    let mut row = GridRow::bare(engine, fixture, status);
    if !matches!(status, Status::Unsupported | Status::Failed) {
        row.mib_per_second = Some(rate);
        row.spread_percent = Some(spread);
        row.steady_mib = Some(12.5);
        row.peak_mib = Some(20.0);
    }
    row
}

/// oneterm, vt100 and avt on three fixtures, one pair per status.
pub(crate) fn sample_results() -> Results {
    let names = ["oneterm", "vt100", "avt"];
    let mut grid = vec![
        // plain_ascii: every engine verified; oneterm fastest, vt100 within the spread.
        row("oneterm", "plain_ascii", Status::Ok, 100.0, 5.0),
        row("vt100", "plain_ascii", Status::Accepted, 97.0, 5.0),
        row("avt", "plain_ascii", Status::Ok, 50.0, 5.0),
        // long_lines: everyone verified too.
        row("oneterm", "long_lines", Status::Ok, 200.0, 2.0),
        row("vt100", "long_lines", Status::Ok, 100.0, 2.0),
        row("avt", "long_lines", Status::Ok, 400.0, 2.0),
        // cjk_wide: one diverged, one failed.
        row("oneterm", "cjk_wide", Status::Ok, 150.0, 2.0),
        row("vt100", "cjk_wide", Status::Diverged, 900.0, 2.0),
        row("avt", "cjk_wide", Status::Failed, 0.0, 0.0),
        // sixel: capability fixture, only oneterm supports it.
        row("oneterm", "sixel", Status::Ok, 40.0, 3.0),
        row("vt100", "sixel", Status::Unsupported, 0.0, 0.0),
        row("avt", "sixel", Status::Unsupported, 0.0, 0.0),
        // grapheme_clusters: ungated.
        row(
            "oneterm",
            "grapheme_clusters",
            Status::Unverified,
            30.0,
            3.0,
        ),
        row("vt100", "grapheme_clusters", Status::Unsupported, 0.0, 0.0),
        row("avt", "grapheme_clusters", Status::Unsupported, 0.0, 0.0),
    ];
    grid[1].first_difference = None;
    grid[7].first_difference = Some("checkpoint 3, rows[4] col 2: expected \"a\" got \"b\"".into());
    grid[8].error = Some("exit 101: panicked".into());
    fill_ratios(&mut grid);
    let engines: Vec<String> = names.iter().map(ToString::to_string).collect();
    let summary = summarize(&grid, &engines);
    Results {
        schema: SCHEMA,
        date: "2026-11-02".into(),
        unpinned: false,
        machine: MachineRecord {
            name: "test-box".into(),
            cpu: "Test CPU".into(),
            logical_cpus: 20,
            ram_gib: 32,
            os: "Test OS".into(),
            power_scheme_before: "Balanced aaa".into(),
            power_scheme_during: "High performance bbb".into(),
            on_ac: Some(true),
            pinned_cpu: Some(4),
            priority: "high".into(),
            power_plan_kept: false,
        },
        toolchains: Toolchains {
            rustc: "1.96.0".into(),
            zig: "0.17.0".into(),
            node: "v22".into(),
            msvc: "17".into(),
        },
        corpus: CorpusRecord {
            version: 1,
            mib: 32,
            lock_sha256: "ff".into(),
        },
        knobs: Knobs {
            cols: 160,
            rows: 45,
            scrollback: 10_000,
            chunk: 65_536,
            warmup: 1,
            passes: 5,
            repetitions: 3,
            checkpoints: 4,
        },
        engines: vec![
            engine("oneterm", &[]),
            engine("vt100", &[]),
            engine("avt", &["+utf8 decode"]),
        ],
        grid,
        summary,
        wall_time_s: 3725,
    }
}

#[test]
fn every_marker_is_distinct_and_the_fastest_verified_cell_is_bold() {
    let results = sample_results();
    let cell = |engine: &str, fixture: &str| {
        let fastest = fastest_verified(&results, fixture);
        grid_cell(results.row(engine, fixture).unwrap(), fastest)
    };
    assert_eq!(cell("oneterm", "plain_ascii"), "**100.0**");
    // vt100 is accepted (†) and within the 5 % spread of the fastest (≈).
    assert_eq!(cell("vt100", "plain_ascii"), "97.0 ≈ †");
    assert_eq!(cell("avt", "plain_ascii"), "50.0");
    assert_eq!(cell("vt100", "cjk_wide"), "900.0 ≠");
    assert_eq!(cell("avt", "cjk_wide"), "fail");
    assert_eq!(cell("vt100", "sixel"), "n/a");
    assert_eq!(cell("oneterm", "grapheme_clusters"), "30.0 ?");
    // A diverged number never counts as the fastest.
    assert_eq!(cell("oneterm", "cjk_wide"), "**150.0**");
}

#[test]
fn geomean_covers_only_the_intersection() {
    let results = sample_results();
    // plain_ascii and long_lines are verified on every engine; cjk_wide is not, and the
    // capability fixtures are never core.
    assert_eq!(results.summary.intersection, ["plain_ascii", "long_lines"]);
    let geomean = &results.summary.geomean_ratio_to_oneterm;
    assert!((geomean["oneterm"] - 1.0).abs() < 1e-9);
    let expected_avt = (0.5f64 * 2.0).sqrt();
    assert!((geomean["avt"] - expected_avt).abs() < 1e-9);
    let expected_vt100 = (0.97f64 * 0.5).sqrt();
    assert!((geomean["vt100"] - expected_vt100).abs() < 1e-9);
}

#[test]
fn status_counts_and_ratios_follow_the_rows() {
    let results = sample_results();
    let counts = &results.summary.status_counts["vt100"];
    assert_eq!(
        (
            counts.ok,
            counts.accepted,
            counts.diverged,
            counts.unsupported,
            counts.failed
        ),
        (1, 1, 1, 2, 0)
    );
    assert_eq!(results.summary.status_counts["avt"].failed, 1);
    assert_eq!(
        results
            .row("oneterm", "plain_ascii")
            .unwrap()
            .ratio_to_oneterm,
        Some(1.0)
    );
    // No ratio for a diverged pair.
    assert_eq!(
        results.row("vt100", "cjk_wide").unwrap().ratio_to_oneterm,
        None
    );
}

#[test]
fn the_markdown_carries_method_tables_and_the_appendix() {
    let text = render_markdown(&sample_results());
    for needle in [
        "# Cross-engine comparison, test-box 2026-11-02",
        "Power: before `Balanced aaa`, during `High performance bbb`",
        "Wall time: 62 min 5 s",
        "## Method",
        "3 repetitions of 1 untimed warm-up and
  5 timed passes: 15 samples",
        "| avt +utf8 decode |",
        "| `plain_ascii` | **100.0** | 97.0 ≈ † | 50.0 | 5 |",
        "| `cjk_wide` | **150.0** | 900.0 ≠ | fail | 2 |",
        "Over 2 core fixtures",
        "| avt +utf8 decode | 1.00 |",
        "| **peak max** | 20.0 ? |",
        "- `vt100` / `cjk_wide` (diverged): checkpoint 3, rows[4] col 2",
        "- `avt` / `cjk_wide` (failed): exit 101: panicked",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
}

#[test]
fn the_unpinned_label_and_missing_geomean_are_stated() {
    let mut results = sample_results();
    results.unpinned = true;
    results.summary.intersection.clear();
    let text = render_markdown(&results);
    assert!(text.contains("UNPINNED"));
    assert!(text.contains("unpinned run"));
    assert!(text.contains("no geomean"));
}

#[test]
fn the_chart_draws_verified_bars_only() {
    let svg = crate::chart::render_svg(&sample_results());
    // 5 verified rows on core fixtures (one accepted, hatched) plus oneterm's capability bar.
    let bars = svg.matches("height=\"11\"").count();
    assert_eq!(bars, 8, "{svg}");
    assert!(svg.contains("fill=\"url(#hatch)\""));
    // Diverged, unsupported, failed and unverified leave labelled gaps instead.
    for gap in ["≠", "n/a", "fail", ">?<"] {
        assert!(svg.contains(gap), "{gap}");
    }
    assert!(!svg.contains("900.0 MiB/s"));
    assert!(!svg.contains("30.0 MiB/s"));
}
