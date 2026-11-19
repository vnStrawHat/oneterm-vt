//! Hand-written SVG line chart of tier-2 throughput across the releases of one in-turns record.

use crate::perf_history::InTurnsRecord;

const WIDTH: f64 = 960.0;
/// Tall enough for the legend of every fixture beside the plot.
const PANEL_HEIGHT: f64 = 470.0;
/// Room below the panel for how the points were measured, and on which machine.
const FOOTER_HEIGHT: f64 = 50.0;
const MARGIN_LEFT: f64 = 70.0;
const MARGIN_RIGHT: f64 = 190.0;
/// From the top of the panel to its plot area (the title and the unit label sit above).
const MARGIN_TOP: f64 = 50.0;
/// From the bottom of the panel up to the plot area (the release labels sit below).
const MARGIN_BOTTOM: f64 = 40.0;

/// One colour per fixture on the white chart: the first ten are the original set, the rest were
/// added for the fixtures that followed. Keep it at least as long as the fixture table.
const PALETTE: [&str; 17] = [
    "#1f77b4", "#d62728", "#2ca02c", "#9467bd", "#ff7f0e", "#17becf", "#8c564b", "#e377c2",
    "#7f7f7f", "#bcbd22", "#111111", "#003f5c", "#e6ab02", "#a0006b", "#66a61e", "#7570b3",
    "#b15928",
];

/// One point of a fixture's line: the build it belongs to, its best round and the range of all
/// its rounds.
struct Point {
    build: usize,
    best: f64,
    slowest: f64,
}

struct Series<'a> {
    fixture: &'a str,
    points: Vec<Point>,
}

/// Renders the chart of one in-turns record: one line per fixture, one point per build that ran
/// it. Every point comes from the same job, so the lines compare the builds and not the
/// machines.
pub(crate) fn render_svg(record: &InTurnsRecord) -> String {
    let height = PANEL_HEIGHT + FOOTER_HEIGHT;
    let mut svg = String::new();
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{height}\" \
         viewBox=\"0 0 {WIDTH} {height}\" font-family=\"sans-serif\" font-size=\"12\">\n"
    ));
    svg.push_str(&format!(
        "<rect width=\"{WIDTH}\" height=\"{height}\" fill=\"#ffffff\"/>\n"
    ));
    draw_panel(&mut svg, record);
    let notes = [
        format!(
            "Every build ran every fixture in turns in one job on {}; a point is its best of {} rounds.",
            record.date, record.rounds
        ),
        record.machine.clone(),
    ];
    for (index, note) in notes.iter().enumerate() {
        svg.push_str(&format!(
            "<text x=\"{MARGIN_LEFT}\" y=\"{:.1}\" fill=\"#444444\">{}</text>\n",
            PANEL_HEIGHT + 14.0 + 18.0 * index as f64,
            escape_xml(note)
        ));
    }
    svg.push_str("</svg>\n");
    svg
}

fn draw_panel(svg: &mut String, record: &InTurnsRecord) {
    let series = collect_series(record);
    let max = series
        .iter()
        .flat_map(|series| series.points.iter().map(|point| point.best))
        .fold(0.0_f64, f64::max);
    let (top, step) = nice_ceiling(max);

    let plot_width = WIDTH - MARGIN_LEFT - MARGIN_RIGHT;
    let plot_height = PANEL_HEIGHT - MARGIN_TOP - MARGIN_BOTTOM;
    let builds = record.builds.len().max(1) as f64;
    let x_of = |index: usize| MARGIN_LEFT + (index as f64 + 0.5) * plot_width / builds;
    let y_of = |value: f64| MARGIN_TOP + plot_height * (1.0 - value / top);

    svg.push_str(&format!(
        "<text x=\"{MARGIN_LEFT}\" y=\"28.0\" font-size=\"16\" fill=\"#222222\">{}</text>\n",
        escape_xml(&format!(
            "oneterm-vt tier 2 grid throughput on {} (MiB/s, higher is better)",
            record.platform
        ))
    ));

    // Y axis: a gridline and a label per step, plus the unit.
    svg.push_str(&format!(
        "<text x=\"{MARGIN_LEFT}\" y=\"{:.1}\" text-anchor=\"end\" fill=\"#444444\">MiB/s</text>\n",
        MARGIN_TOP - 10.0
    ));
    let step_count = (top / step).round() as usize;
    for index in 0..=step_count {
        let value = index as f64 * step;
        let y = y_of(value);
        svg.push_str(&format!(
            "<line x1=\"{MARGIN_LEFT}\" y1=\"{y:.1}\" x2=\"{:.1}\" y2=\"{y:.1}\" stroke=\"#dddddd\"/>\n",
            WIDTH - MARGIN_RIGHT
        ));
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\" fill=\"#444444\">{value:.1}</text>\n",
            MARGIN_LEFT - 8.0,
            y + 4.0
        ));
    }

    // X axis: one category per build.
    for (index, build) in record.builds.iter().enumerate() {
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" fill=\"#444444\">v{}</text>\n",
            x_of(index),
            MARGIN_TOP + plot_height + 20.0,
            escape_xml(&build.version)
        ));
    }

    // Series: line first, then points on top.
    for (index, series) in series.iter().enumerate() {
        let colour = PALETTE[index % PALETTE.len()];
        if series.points.len() >= 2 {
            let coordinates: Vec<String> = series
                .points
                .iter()
                .map(|point| format!("{:.1},{:.1}", x_of(point.build), y_of(point.best)))
                .collect();
            svg.push_str(&format!(
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"2\"/>\n",
                coordinates.join(" ")
            ));
        }
        for point in &series.points {
            svg.push_str(&format!(
                "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"3\" fill=\"{colour}\"><title>{} v{}: {:.1} MiB/s (rounds from {:.1})</title></circle>\n",
                x_of(point.build),
                y_of(point.best),
                escape_xml(series.fixture),
                escape_xml(&record.builds[point.build].version),
                point.best,
                point.slowest
            ));
        }
    }

    // Legend in the right margin.
    for (index, series) in series.iter().enumerate() {
        let y = MARGIN_TOP + 21.0 * index as f64;
        let x = WIDTH - MARGIN_RIGHT + 20.0;
        svg.push_str(&format!(
            "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"12\" height=\"12\" fill=\"{}\"/>\n",
            PALETTE[index % PALETTE.len()]
        ));
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"#222222\">{}</text>\n",
            x + 18.0,
            y + 10.0,
            escape_xml(series.fixture)
        ));
    }
}

/// Groups the points by fixture, in first-seen order across builds.
fn collect_series(record: &InTurnsRecord) -> Vec<Series<'_>> {
    let mut all: Vec<Series<'_>> = Vec::new();
    for (build, measured) in record.builds.iter().enumerate() {
        for row in &measured.tier2_grid {
            let point = Point {
                build,
                best: row.mib_per_second,
                slowest: row
                    .rounds
                    .iter()
                    .copied()
                    .fold(row.mib_per_second, f64::min),
            };
            match all.iter_mut().find(|series| series.fixture == row.fixture) {
                Some(series) => series.points.push(point),
                None => all.push(Series {
                    fixture: &row.fixture,
                    points: vec![point],
                }),
            }
        }
    }
    all
}

/// Returns `(top, step)` for the Y axis: about five steps, step in `{1, 2, 2.5, 5, 10} x 10^k`.
fn nice_ceiling(max: f64) -> (f64, f64) {
    if max <= 0.0 {
        return (1.0, 0.2);
    }
    let raw_step = max / 5.0;
    let magnitude = 10.0_f64.powf(raw_step.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|factor| factor * magnitude)
        .find(|candidate| *candidate >= raw_step)
        .unwrap_or(10.0 * magnitude);
    (((max / step) - 1e-9).ceil() * step, step)
}

fn escape_xml(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perf_history::{BestOfRounds, BuildThroughput};

    /// One build with `(fixture, best MiB/s)` rows; every fixture had a slower round at half.
    fn build(version: &str, fixtures: &[(&str, f64)]) -> BuildThroughput {
        BuildThroughput {
            version: version.to_owned(),
            tier2_grid: fixtures
                .iter()
                .map(|(fixture, best)| BestOfRounds {
                    fixture: (*fixture).to_owned(),
                    mib_per_second: *best,
                    rounds: vec![best / 2.0, *best],
                })
                .collect(),
        }
    }

    fn record(builds: Vec<BuildThroughput>) -> InTurnsRecord {
        InTurnsRecord {
            engine: "oneterm-vt".to_owned(),
            date: "2026-11-19".to_owned(),
            platform: "linux".to_owned(),
            machine: "GitHub <runner>, 4 vCPU".to_owned(),
            columns: 160,
            lines: 45,
            mib_per_fixture: 32,
            rounds: 2,
            runs: 5,
            builds,
        }
    }

    #[test]
    fn every_fixture_gets_its_own_colour() {
        let mut colours: Vec<&str> = PALETTE.to_vec();
        colours.sort_unstable();
        colours.dedup();
        assert_eq!(colours.len(), PALETTE.len());
        assert!(PALETTE.len() >= crate::fixtures::FIXTURES.len());
    }

    #[test]
    fn the_legend_of_every_fixture_fits_beside_the_plot() {
        let legend = 21.0 * crate::fixtures::FIXTURES.len() as f64;
        assert!(legend <= PANEL_HEIGHT - MARGIN_TOP - MARGIN_BOTTOM);
    }

    #[test]
    fn two_builds_two_fixtures_give_two_lines_four_points() {
        let fixtures = [("plain", 70.0), ("sgr", 200.0)];
        let svg = render_svg(&record(vec![
            build("0.1.0", &fixtures),
            build("0.2.0", &fixtures),
        ]));
        assert_eq!(svg.matches("<polyline").count(), 2);
        assert_eq!(svg.matches("<circle").count(), 4);
        assert!(svg.contains(">v0.1.0</text>") && svg.contains(">v0.2.0</text>"));
        assert!(svg.contains("<title>sgr v0.2.0: 200.0 MiB/s (rounds from 100.0)</title>"));
    }

    #[test]
    fn one_build_has_points_but_no_lines() {
        let svg = render_svg(&record(vec![build(
            "0.1.0",
            &[("plain", 70.0), ("sgr", 200.0)],
        )]));
        assert_eq!(svg.matches("<polyline").count(), 0);
        assert_eq!(svg.matches("<circle").count(), 2);
    }

    #[test]
    fn a_fixture_one_build_lacks_has_no_point_there() {
        let svg = render_svg(&record(vec![
            build("0.1.0", &[("plain", 70.0)]),
            build("0.2.0", &[("plain", 72.0), ("sgr", 200.0)]),
            build("0.3.0", &[("plain", 71.0), ("sgr", 210.0)]),
        ]));
        assert_eq!(svg.matches("<circle").count(), 5);
        assert_eq!(svg.matches("<polyline").count(), 2);
        assert!(!svg.contains("sgr v0.1.0"));
    }

    #[test]
    fn names_are_escaped() {
        let svg = render_svg(&record(vec![build("0.1.0", &[("a<b&c", 1.0)])]));
        assert!(svg.contains("a&lt;b&amp;c"));
        assert!(!svg.contains("a<b&c"));
        assert!(svg.contains("GitHub &lt;runner&gt;, 4 vCPU"));
    }

    #[test]
    fn title_and_footer_say_what_was_measured_and_how() {
        let svg = render_svg(&record(vec![build("0.1.0", &[("plain", 70.0)])]));
        assert!(
            svg.contains("oneterm-vt tier 2 grid throughput on linux (MiB/s, higher is better)")
        );
        assert_eq!(svg.matches(">MiB/s</text>").count(), 1);
        assert!(svg.contains(
            "Every build ran every fixture in turns in one job on 2026-11-19; a point is its best of 2 rounds."
        ));
        assert!(svg.contains("height=\"520\""));
    }

    #[test]
    fn a_record_without_builds_is_an_empty_chart() {
        let svg = render_svg(&record(Vec::new()));
        assert_eq!(svg.matches("<circle").count(), 0);
        assert!(svg.ends_with("</svg>\n"));
    }

    #[test]
    fn nice_ceiling_survives_float_error() {
        let (top, _) = nice_ceiling(0.3);
        assert!((top - 0.3).abs() < 1e-9, "top was {top}");
    }

    #[test]
    fn nice_ceiling_picks_round_steps() {
        assert_eq!(nice_ceiling(74.9), (80.0, 20.0));
        assert_eq!(nice_ceiling(226.5), (250.0, 50.0));
        assert_eq!(nice_ceiling(0.0), (1.0, 0.2));
    }
}
