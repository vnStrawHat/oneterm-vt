//! The grid chart (output.md "Chart"): hand-written SVG, one panel per fixture class, one bar
//! per verified engine on a log-scale MiB/s axis.

use std::fmt::Write as _;

use crate::corpus::{self, Class, FixtureInfo};
use crate::engines::ORDER;
use crate::results::Results;
use crate::verify::Status;

const WIDTH: f64 = 1100.0;
const LEFT: f64 = 190.0;
const RIGHT: f64 = 30.0;
const BAR: f64 = 11.0;
const GROUP_GAP: f64 = 9.0;

/// One colour per engine in the fixed engine order; distinct under common colour-blindness.
const PALETTE: [&str; 9] = [
    "#0072B2", "#E69F00", "#009E73", "#CC79A7", "#56B4E9", "#D55E00", "#7F7F7F", "#6A3D9A",
    "#B15928",
];

pub fn engine_color(name: &str) -> &'static str {
    let index = ORDER.iter().position(|e| *e == name).unwrap_or(0);
    PALETTE[index % PALETTE.len()]
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The marker a missing bar shows.
fn gap_label(status: Status) -> &'static str {
    match status {
        Status::Diverged => "≠",
        Status::Unverified => "?",
        Status::Unsupported => "n/a",
        _ => "fail",
    }
}

/// Decade bounds `(low, high)` of the axis, covering every drawn bar.
fn axis_range(results: &Results) -> (f64, f64) {
    let rates: Vec<f64> = results
        .grid
        .iter()
        .filter(|r| r.status.is_verified())
        .filter_map(|r| r.mib_per_second)
        .filter(|v| *v > 0.0)
        .collect();
    let low = rates.iter().copied().fold(f64::MAX, f64::min);
    let high = rates.iter().copied().fold(0.0, f64::max);
    if rates.is_empty() {
        return (1.0, 10.0);
    }
    let low = 10f64.powf(low.log10().floor());
    let high = 10f64.powf(high.log10().ceil()).max(low * 10.0);
    (low, high)
}

fn x_of(value: f64, low: f64, high: f64) -> f64 {
    let t = (value.max(low).log10() - low.log10()) / (high.log10() - low.log10());
    LEFT + t * (WIDTH - LEFT - RIGHT)
}

struct Panel<'a> {
    title: &'static str,
    fixtures: Vec<&'a FixtureInfo>,
}

fn panels(results: &Results) -> Vec<Panel<'_>> {
    let present = |f: &&FixtureInfo| results.grid.iter().any(|r| r.fixture == f.name);
    [
        (Class::Core, "core fixtures"),
        (Class::Capability, "capability fixtures"),
    ]
    .into_iter()
    .map(|(class, title)| Panel {
        title,
        fixtures: corpus::FIXTURES
            .iter()
            .filter(|f| f.class == class)
            .filter(present)
            .collect(),
    })
    .filter(|p| !p.fixtures.is_empty())
    .collect()
}

pub fn render_svg(results: &Results) -> String {
    let names = results.engine_names();
    let (low, high) = axis_range(results);
    let group_height = names.len() as f64 * (BAR + 1.0) + GROUP_GAP;
    let panels = panels(results);
    let mut y = 76.0;
    let mut body = String::new();
    for panel in &panels {
        let _ = writeln!(
            body,
            r#"<text x="{LEFT}" y="{y}" class="panel">{}</text>"#,
            panel.title
        );
        y += 14.0;
        let top = y;
        for fixture in &panel.fixtures {
            let _ = writeln!(
                body,
                r#"<text x="{}" y="{}" class="label" text-anchor="end">{}</text>"#,
                LEFT - 8.0,
                y + BAR + 1.0,
                fixture.name
            );
            for (slot, name) in names.iter().enumerate() {
                let by = y + slot as f64 * (BAR + 1.0);
                let Some(row) = results.row(name, fixture.name) else {
                    continue;
                };
                match (row.status.is_verified(), row.mib_per_second) {
                    (true, Some(rate)) => {
                        let fill = if row.status == Status::Accepted {
                            "url(#hatch)".to_string()
                        } else {
                            engine_color(name).to_string()
                        };
                        let x = x_of(rate, low, high);
                        let _ = writeln!(
                            body,
                            r#"<rect x="{LEFT}" y="{by:.1}" width="{:.1}" height="{BAR}" fill="{fill}" stroke="{}"><title>{name} {rate:.1} MiB/s</title></rect>"#,
                            (x - LEFT).max(1.0),
                            engine_color(name)
                        );
                    }
                    _ => {
                        let _ = writeln!(
                            body,
                            r#"<text x="{}" y="{:.1}" class="gap">{}</text>"#,
                            LEFT + 3.0,
                            by + BAR - 2.0,
                            gap_label(row.status)
                        );
                    }
                }
            }
            y += group_height;
        }
        // Axis ticks and grid lines for this panel.
        let mut tick = low;
        while tick <= high * 1.0001 {
            let x = x_of(tick, low, high);
            let _ = writeln!(
                body,
                r##"<line x1="{x:.1}" y1="{top:.1}" x2="{x:.1}" y2="{:.1}" class="grid"/>"##,
                y - GROUP_GAP
            );
            let _ = writeln!(
                body,
                r#"<text x="{x:.1}" y="{:.1}" class="tick" text-anchor="middle">{tick}</text>"#,
                y - GROUP_GAP + 12.0
            );
            tick *= 10.0;
        }
        y += 34.0;
    }
    let height = y;
    let mut svg = String::new();
    let _ = writeln!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {WIDTH} {height:.0}" width="{WIDTH}" height="{height:.0}" font-family="Segoe UI, Helvetica, Arial, sans-serif">
<style>
.title {{ font-size: 15px; font-weight: 600; fill: #1a1a1a }}
.panel {{ font-size: 13px; font-weight: 600; fill: #1a1a1a }}
.label {{ font-size: 11px; fill: #1a1a1a }}
.tick {{ font-size: 10px; fill: #555 }}
.gap {{ font-size: 10px; fill: #777 }}
.legend {{ font-size: 11px; fill: #1a1a1a }}
.grid {{ stroke: #ddd; stroke-width: 1 }}
</style>
<defs><pattern id="hatch" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="4" height="4" fill="#fff"/><rect width="2" height="4" fill="#555"/></pattern></defs>
<rect width="100%" height="100%" fill="#ffffff"/>"##
    );
    let _ = writeln!(
        svg,
        r#"<text x="{LEFT}" y="22" class="title">Grid throughput (MiB/s, log scale), {} {}, corpus v{}</text>"#,
        escape(&results.machine.name),
        escape(&results.date),
        results.corpus.version
    );
    // Legend: engine and version, two rows of five.
    for (i, name) in names.iter().enumerate() {
        let version = results
            .engines
            .iter()
            .find(|e| e.name == *name)
            .map_or(String::new(), |e| e.version.clone());
        let lx = LEFT + (i % 5) as f64 * 180.0;
        let ly = 38.0 + (i / 5) as f64 * 16.0;
        let _ = writeln!(
            svg,
            r#"<rect x="{lx}" y="{}" width="10" height="10" fill="{}"/><text x="{}" y="{}" class="legend">{} {}</text>"#,
            ly - 9.0,
            engine_color(name),
            lx + 14.0,
            ly,
            escape(name),
            escape(&version)
        );
    }
    svg.push_str(&body);
    svg.push_str("</svg>\n");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_has_its_own_colour() {
        let mut colours: Vec<&str> = ORDER.iter().map(|e| engine_color(e)).collect();
        colours.sort_unstable();
        colours.dedup();
        assert_eq!(colours.len(), ORDER.len());
    }

    #[test]
    fn the_axis_is_log_between_decades() {
        let (low, high) = (1.0, 1000.0);
        assert!((x_of(1.0, low, high) - LEFT).abs() < 1e-9);
        assert!((x_of(1000.0, low, high) - (WIDTH - RIGHT)).abs() < 1e-9);
        let mid = (x_of(10.0, low, high) + x_of(100.0, low, high)) / 2.0;
        assert!((x_of(31.622_776_6, low, high) - mid).abs() < 1e-3);
    }
}
