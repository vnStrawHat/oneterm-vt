//! The markdown report (output.md "Markdown"): methodology block, engines, grid table with
//! markers, memory table and the appendix of diverged and failed pairs.

use std::fmt::Write as _;

use crate::corpus::{self, FixtureInfo};
use crate::results::{GridRow, Results};
use crate::verify::Status;

/// `n/a` or `fail` for rows without a number; otherwise the figure with its markers.
/// `fastest` is the fastest verified row of the fixture, the one in bold.
pub fn grid_cell(row: &GridRow, fastest: Option<&GridRow>) -> String {
    match row.status {
        Status::Unsupported => return "n/a".to_string(),
        Status::Failed => return "fail".to_string(),
        _ => {}
    }
    let Some(rate) = row.mib_per_second else {
        return "fail".to_string();
    };
    let mut text = format!("{rate:.1}");
    match row.status {
        Status::Diverged => text.push_str(" ≠"),
        Status::Unverified => text.push_str(" ?"),
        _ => {
            let is_fastest = fastest.is_some_and(|f| f.engine == row.engine);
            if is_fastest {
                text = format!("**{text}**");
            } else if fastest.is_some_and(|f| is_tie(f, row)) {
                text.push_str(" ≈");
            }
            if row.status == Status::Accepted {
                text.push_str(" †");
            }
        }
    }
    text
}

/// A tie: the gap to the fastest is smaller than the larger of the two spreads.
fn is_tie(fastest: &GridRow, row: &GridRow) -> bool {
    let (Some(top), Some(mine)) = (fastest.mib_per_second, row.mib_per_second) else {
        return false;
    };
    let gap = (top - mine) / top * 100.0;
    let spread = fastest
        .spread_percent
        .unwrap_or(0.0)
        .max(row.spread_percent.unwrap_or(0.0));
    gap < spread
}

/// The fastest `ok` or `accepted` row of a fixture.
pub fn fastest_verified<'a>(results: &'a Results, fixture: &str) -> Option<&'a GridRow> {
    results
        .grid
        .iter()
        .filter(|r| r.fixture == fixture && r.status.is_verified())
        .filter(|r| r.mib_per_second.is_some())
        .max_by(|a, b| {
            a.mib_per_second
                .partial_cmp(&b.mib_per_second)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn engine_label(results: &Results, name: &str) -> String {
    match results.engines.iter().find(|e| e.name == name) {
        Some(e) if !e.labels.is_empty() => format!("{name} {}", e.labels.join(" ")),
        _ => name.to_string(),
    }
}

fn minutes(seconds: u64) -> String {
    format!("{} min {} s", seconds / 60, seconds % 60)
}

const METHOD: &str = "\
## Method

- Layer: grid. Bytes go in 64 KiB chunks through each engine's public feed call into full
  terminal state (grid, scrollback, modes, styles, links). No rendering, no PTY. Memory is the
  private bytes of the verify process, informational.
- One process per (engine, fixture, repetition); {repetitions} repetitions of {warmup} untimed warm-up and
  {passes} timed passes: {samples} samples per figure, median reported. A fresh terminal is built before
  each timer and dropped after it; the timer includes the engine's `finish` (deferred-work flush).
- MiB/s = input bytes / median pass time (MiB = 2^20). The spread is (p90 - p10) / median over
  the {samples} samples.
- Every number comes with a verified end state: a verify pass dumps the canonical state at 4
  checkpoints and compares it with the golden; the timed passes must end in the same state.
- Goldens are the dump set shared by at least 3 engines (oneterm-vt is one voter). A fixture
  without that consensus has no golden and every engine is `?` there.
- Knobs: 160x45, 10 000 scrollback lines, same corpus bytes for every engine, baseline x86-64.
- Markers: `≈` tie with the fastest (gap below the larger spread), `†` accepted divergence,
  `≠` diverged (never charted), `?` unverified (no golden, or memory), `n/a` capability not
  supported, `fail` crash, timeout or inconsistent end state. Bold: fastest verified cell.
- Geomean and median: only over the core fixtures on which every engine of the run is `ok` or
  `accepted`.
";

/// The methodology block with this run's repetition counts filled in.
fn method(results: &Results) -> String {
    let k = &results.knobs;
    METHOD
        .replace("{repetitions}", &k.repetitions.to_string())
        .replace("{warmup}", &k.warmup.to_string())
        .replace("{passes}", &k.passes.to_string())
        .replace("{samples}", &(k.repetitions * k.passes).to_string())
}

fn push_machine(out: &mut String, results: &Results) {
    let m = &results.machine;
    let pinned = match (results.unpinned, m.pinned_cpu) {
        (false, Some(cpu)) => format!("pinned to logical CPU {cpu}"),
        _ => "UNPINNED".to_string(),
    };
    let _ = writeln!(
        out,
        "- Machine `{}`: {}, {} logical CPUs, {} GiB RAM, {}; {pinned}, {} priority.",
        m.name, m.cpu, m.logical_cpus, m.ram_gib, m.os, m.priority
    );
    let ac = match m.on_ac {
        Some(true) => "on AC",
        Some(false) => "ON BATTERY",
        None => "AC status unknown",
    };
    let kept = if m.power_plan_kept {
        " (kept: --keep-power-plan)"
    } else {
        ""
    };
    let _ = writeln!(
        out,
        "- Power: before `{}`, during `{}`{kept}; {ac}.",
        m.power_scheme_before, m.power_scheme_during
    );
    let t = &results.toolchains;
    let _ = writeln!(
        out,
        "- Toolchains: rustc {}, zig {}, node {}, MSVC {}.",
        t.rustc, t.zig, t.node, t.msvc
    );
    let k = &results.knobs;
    let _ = writeln!(
        out,
        "- Knobs: {}x{}, scrollback {}, chunk {} B, {} repetitions x ({} warm-up + {} passes), {} checkpoints.",
        k.cols, k.rows, k.scrollback, k.chunk, k.repetitions, k.warmup, k.passes, k.checkpoints
    );
    let _ = writeln!(
        out,
        "- Corpus v{}: {} MiB per fixture, lock sha256 `{}`.",
        results.corpus.version, results.corpus.mib, results.corpus.lock_sha256
    );
    let _ = writeln!(out, "- Wall time: {}.", minutes(results.wall_time_s));
    if results.unpinned {
        let _ = writeln!(
            out,
            "\n> **unpinned run**: never compare these figures with a pinned run."
        );
    }
    if results.engines.iter().any(|e| e.name == "xterm") {
        let _ = writeln!(
            out,
            "\nxterm.js: V8 JIT and GC helper threads share the pinned core with its main thread."
        );
    }
}

fn push_engines(out: &mut String, results: &Results) {
    out.push_str("\n## Engines\n\n| engine | version / pin | language | build | capabilities |\n|---|---|---|---|---|\n");
    for e in &results.engines {
        let _ = writeln!(
            out,
            "| {} | {} ({}) | {} | {} | {} |",
            engine_label(results, &e.name),
            e.version,
            e.source,
            e.language,
            e.build,
            if e.capabilities.is_empty() {
                "-".to_string()
            } else {
                e.capabilities.join(", ")
            }
        );
    }
}

fn push_grid(out: &mut String, results: &Results) {
    let names = results.engine_names();
    out.push_str("\n## Grid (MiB/s)\n\n| fixture |");
    for name in &names {
        let _ = write!(out, " {} |", engine_label(results, name));
    }
    out.push_str(" spread max % |\n|---|");
    out.push_str(&"---:|".repeat(names.len() + 1));
    out.push('\n');
    let fixtures: Vec<&FixtureInfo> = corpus::FIXTURES
        .iter()
        .filter(|f| results.grid.iter().any(|r| r.fixture == f.name))
        .collect();
    for fixture in &fixtures {
        let fastest = fastest_verified(results, fixture.name);
        let _ = write!(out, "| `{}` |", fixture.name);
        let mut spread_max: f64 = 0.0;
        for name in &names {
            match results.row(name, fixture.name) {
                Some(row) => {
                    spread_max = spread_max.max(row.spread_percent.unwrap_or(0.0));
                    let _ = write!(out, " {} |", grid_cell(row, fastest));
                }
                None => out.push_str(" - |"),
            }
        }
        let _ = writeln!(out, " {spread_max:.0} |");
    }
    out.push_str("\n### Status counts\n\n| engine | ok | accepted | diverged | unverified | n/a | fail |\n|---|---:|---:|---:|---:|---:|---:|\n");
    for name in &names {
        if let Some(c) = results.summary.status_counts.get(*name) {
            let _ = writeln!(
                out,
                "| {name} | {} | {} | {} | {} | {} | {} |",
                c.ok, c.accepted, c.diverged, c.unverified, c.unsupported, c.failed
            );
        }
    }
    out.push_str("\n### Geomean of ratio to oneterm-vt\n\n");
    let summary = &results.summary;
    if summary.intersection.is_empty() {
        out.push_str(
            "No core fixture is `ok` or `accepted` on every engine of this run: no geomean.\n",
        );
        return;
    }
    let _ = writeln!(
        out,
        "Over {} core fixtures on which every engine is `ok` or `accepted`: {}.\n",
        summary.intersection.len(),
        summary
            .intersection
            .iter()
            .map(|f| format!("`{f}`"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    out.push_str(
        "The median is the middle fixture's ratio: a single fixture with an extreme ratio moves the \
         geomean, not the median. Read both.\n\n",
    );
    out.push_str("| engine | geomean ratio | median ratio |\n|---|---:|---:|\n");
    for name in &names {
        if let Some(ratio) = summary.geomean_ratio_to_oneterm.get(*name) {
            // A results file written before the median existed has none.
            let median = summary
                .median_ratio_to_oneterm
                .get(*name)
                .map_or_else(|| "-".to_owned(), |median| format!("{median:.2}"));
            let _ = writeln!(
                out,
                "| {} | {ratio:.2} | {median} |",
                engine_label(results, name)
            );
        }
    }
}

fn push_memory(out: &mut String, results: &Results) {
    let names = results.engine_names();
    out.push_str(
        "\n## Memory (private MiB after the verify pass, informational `?`)\n\n| fixture |",
    );
    for name in &names {
        let _ = write!(out, " {name} |");
    }
    out.push_str("\n|---|");
    out.push_str(&"---:|".repeat(names.len()));
    out.push('\n');
    for fixture in corpus::FIXTURES
        .iter()
        .filter(|f| results.grid.iter().any(|r| r.fixture == f.name))
    {
        let _ = write!(out, "| `{}` |", fixture.name);
        for name in &names {
            let cell = match results.row(name, fixture.name) {
                Some(row) if row.status == Status::Unsupported => "n/a".to_string(),
                Some(row) => row
                    .steady_mib
                    .map_or("fail".to_string(), |m| format!("{m:.1} ?")),
                None => "-".to_string(),
            };
            let _ = write!(out, " {cell} |");
        }
        out.push('\n');
    }
    out.push_str("| **peak max** |");
    for name in &names {
        let peak = results
            .grid
            .iter()
            .filter(|r| r.engine == *name)
            .filter_map(|r| r.peak_mib)
            .fold(None, |acc: Option<f64>, m| {
                Some(acc.map_or(m, |a| a.max(m)))
            });
        let _ = write!(
            out,
            " {} |",
            peak.map_or("-".to_string(), |m| format!("{m:.1} ?"))
        );
    }
    out.push('\n');
}

fn push_appendix(out: &mut String, results: &Results) {
    let problems: Vec<&GridRow> = results
        .grid
        .iter()
        .filter(|r| matches!(r.status, Status::Diverged | Status::Failed))
        .collect();
    out.push_str("\n## Appendix: diverged and failed pairs\n\n");
    if problems.is_empty() {
        out.push_str("None.\n");
        return;
    }
    for row in problems {
        let detail = row
            .first_difference
            .as_deref()
            .or(row.error.as_deref())
            .unwrap_or("no detail recorded");
        let _ = writeln!(
            out,
            "- `{}` / `{}` ({}): {detail}",
            row.engine,
            row.fixture,
            row.status.as_str()
        );
    }
}

/// The whole `.md` of a run.
pub fn render_markdown(results: &Results) -> String {
    let mut out = format!(
        "# Cross-engine comparison, {} {}\n\n",
        results.machine.name, results.date
    );
    push_machine(&mut out, results);
    out.push('\n');
    out.push_str(&method(results));
    push_engines(&mut out, results);
    push_grid(&mut out, results);
    push_memory(&mut out, results);
    push_appendix(&mut out, results);
    out
}

#[cfg(test)]
#[path = "report_tests.rs"]
pub(crate) mod report_tests;
