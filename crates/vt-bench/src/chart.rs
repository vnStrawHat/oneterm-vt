//! Hand-written SVG line chart of tier-2 throughput across releases.

use anyhow::{Result, bail};

use crate::perf_history::Release;

const WIDTH: f64 = 960.0;
const HEIGHT: f64 = 540.0;
const MARGIN_LEFT: f64 = 70.0;
const MARGIN_RIGHT: f64 = 190.0;
const MARGIN_TOP: f64 = 50.0;
const MARGIN_BOTTOM: f64 = 60.0;

const PALETTE: [&str; 10] = [
    "#1f77b4", "#d62728", "#2ca02c", "#9467bd", "#ff7f0e", "#17becf", "#8c564b", "#e377c2",
    "#7f7f7f", "#bcbd22",
];

/// One fixture's points: `(release index, MiB/s, spread %)`.
struct Series<'a> {
    name: &'a str,
    points: Vec<(usize, f64, f64)>,
}

/// Renders the chart. `releases` must already be sorted ascending by version and come from one
/// platform: throughput is never compared across platforms.
pub(crate) fn render_svg(releases: &[Release]) -> Result<String> {
    let mut platforms: Vec<&str> = releases
        .iter()
        .map(|release| release.record.platform.as_str())
        .collect();
    platforms.sort_unstable();
    platforms.dedup();
    let [platform] = platforms.as_slice() else {
        bail!(
            "chart needs records from one platform, found: {}",
            platforms.join(", ")
        );
    };
    let series = collect_series(releases);
    let max = series
        .iter()
        .flat_map(|series| series.points.iter().map(|point| point.1))
        .fold(0.0_f64, f64::max);
    let (top, step) = nice_ceiling(max);

    let plot_width = WIDTH - MARGIN_LEFT - MARGIN_RIGHT;
    let plot_height = HEIGHT - MARGIN_TOP - MARGIN_BOTTOM;
    let x_of =
        |index: usize| MARGIN_LEFT + (index as f64 + 0.5) * plot_width / releases.len() as f64;
    let y_of = |value: f64| MARGIN_TOP + plot_height * (1.0 - value / top);

    let mut svg = String::new();
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{HEIGHT}\" \
         viewBox=\"0 0 {WIDTH} {HEIGHT}\" font-family=\"sans-serif\" font-size=\"12\">\n"
    ));
    svg.push_str(&format!(
        "<rect width=\"{WIDTH}\" height=\"{HEIGHT}\" fill=\"#ffffff\"/>\n"
    ));
    svg.push_str(&format!(
        "<text x=\"{MARGIN_LEFT}\" y=\"28\" font-size=\"16\" fill=\"#222222\">{}</text>\n",
        escape_xml(&format!(
            "oneterm-vt tier 2 throughput on {platform} (MiB/s, higher is better)"
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

    // X axis: one category per release.
    for (index, release) in releases.iter().enumerate() {
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" fill=\"#444444\">v{}</text>\n",
            x_of(index),
            HEIGHT - MARGIN_BOTTOM + 20.0,
            release.version
        ));
    }

    // Series: line first, then points on top.
    for (index, series) in series.iter().enumerate() {
        let colour = PALETTE[index % PALETTE.len()];
        if series.points.len() >= 2 {
            let coordinates: Vec<String> = series
                .points
                .iter()
                .map(|&(release, value, _)| format!("{:.1},{:.1}", x_of(release), y_of(value)))
                .collect();
            svg.push_str(&format!(
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"2\"/>\n",
                coordinates.join(" ")
            ));
        }
        for &(release, value, spread) in &series.points {
            svg.push_str(&format!(
                "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"3\" fill=\"{colour}\"><title>{} v{}: {value:.1} MiB/s (spread {spread:.1}%)</title></circle>\n",
                x_of(release),
                y_of(value),
                escape_xml(series.name),
                releases[release].version
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
            escape_xml(series.name)
        ));
    }

    let mut machines: Vec<&str> = releases
        .iter()
        .map(|release| release.record.machine.as_str())
        .collect();
    machines.sort_unstable();
    machines.dedup();
    if machines.len() > 1 {
        svg.push_str(&format!(
            "<text x=\"{MARGIN_LEFT}\" y=\"{:.1}\" fill=\"#b00020\">{}</text>\n",
            HEIGHT - 12.0,
            escape_xml(&format!(
                "warning: {} machines in this history; compare runs from one machine only",
                machines.len()
            ))
        ));
    }
    svg.push_str("</svg>\n");
    Ok(svg)
}

/// Groups points by fixture, in first-seen order across releases.
fn collect_series(releases: &[Release]) -> Vec<Series<'_>> {
    let mut all: Vec<Series<'_>> = Vec::new();
    for (index, release) in releases.iter().enumerate() {
        for sample in &release.record.tiers.tier2_grid {
            let point = (index, sample.mib_per_second, sample.spread_percent);
            match all.iter_mut().find(|series| series.name == sample.fixture) {
                Some(series) => series.points.push(point),
                None => all.push(Series {
                    name: &sample.fixture,
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
    use crate::perf_history::parse_release;

    #[test]
    fn nice_ceiling_survives_float_error() {
        let (top, _) = nice_ceiling(0.3);
        assert!((top - 0.3).abs() < 1e-9, "top was {top}");
    }

    /// `fixtures` is `(name, MiB/s)` pairs.
    fn release(version: &str, machine: &str, fixtures: &[(&str, f64)]) -> Release {
        let samples: Vec<String> = fixtures
            .iter()
            .map(|(name, mib)| {
                format!(
                    r#"{{"fixture":"{name}","mib_per_second":{mib},"ns_per_byte":1.0,"spread_percent":2.0}}"#
                )
            })
            .collect();
        let json = format!(
            r#"{{"version":"{version}","platform":"linux","machine":"{machine}","tiers":{{"tier2_grid":[{}]}}}}"#,
            samples.join(",")
        );
        parse_release("test", &json).unwrap()
    }

    #[test]
    fn two_releases_two_fixtures_give_two_lines_four_points() {
        let fixtures = [("plain", 70.0), ("sgr", 200.0)];
        let svg = render_svg(&[
            release("0.1.0", "m", &fixtures),
            release("0.2.0", "m", &fixtures),
        ])
        .unwrap();
        assert_eq!(svg.matches("<polyline").count(), 2);
        assert_eq!(svg.matches("<circle").count(), 4);
    }

    #[test]
    fn one_release_has_points_but_no_lines() {
        let svg = render_svg(&[release("0.1.0", "m", &[("plain", 70.0), ("sgr", 200.0)])]).unwrap();
        assert_eq!(svg.matches("<polyline").count(), 0);
        assert_eq!(svg.matches("<circle").count(), 2);
    }

    #[test]
    fn fixture_names_are_escaped() {
        let svg = render_svg(&[release("0.1.0", "m", &[("a<b&c", 1.0)])]).unwrap();
        assert!(svg.contains("a&lt;b&amp;c"));
        assert!(!svg.contains("a<b&c"));
    }

    #[test]
    fn machine_note_appears_only_for_mixed_machines() {
        let fixtures = [("plain", 70.0)];
        let mixed = render_svg(&[
            release("0.1.0", "one", &fixtures),
            release("0.2.0", "two", &fixtures),
        ])
        .unwrap();
        assert!(mixed.contains("warning: 2 machines in this history"));
        let single = render_svg(&[
            release("0.1.0", "one", &fixtures),
            release("0.2.0", "one", &fixtures),
        ])
        .unwrap();
        assert!(!single.contains("warning:"));
    }

    #[test]
    fn title_names_the_platform() {
        let svg = render_svg(&[release("0.1.0", "m", &[("plain", 70.0)])]).unwrap();
        assert!(svg.contains("throughput on linux"));
    }

    #[test]
    fn mixed_platforms_are_refused() {
        let mut foreign = release("0.2.0", "m", &[("plain", 70.0)]);
        foreign.record.platform = "windows".to_owned();
        let error = render_svg(&[release("0.1.0", "m", &[("plain", 70.0)]), foreign])
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            "chart needs records from one platform, found: linux, windows"
        );
    }

    #[test]
    fn nice_ceiling_picks_round_steps() {
        assert_eq!(nice_ceiling(74.9), (80.0, 20.0));
        assert_eq!(nice_ceiling(226.5), (250.0, 50.0));
        assert_eq!(nice_ceiling(0.0), (1.0, 0.2));
    }
}
