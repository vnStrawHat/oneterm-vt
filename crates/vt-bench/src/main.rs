//! `vt-bench`: benchmark tiers and the perf-history chart for `oneterm-vt`.

mod chart;
mod perf_history;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

const USAGE: &str = "\
vt-bench <command> [flags]

Measure (lands phase by phase; see the roadmap):
  all | parser | grid | render | resize | rss | fixtures
      [--mib N] [--frames N] [--json] [--out FILE] [--machine TEXT]
  grid --check [--baseline FILE] [--tolerance R]

Report:
  chart --history DIR --out FILE   SVG of tier-2 MiB/s per release, one line per fixture

  help | --help | -h               print this text
";

const MEASURE_COMMANDS: [&str; 7] = [
    "all", "parser", "grid", "render", "resize", "rss", "fixtures",
];

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
        _ if MEASURE_COMMANDS.contains(&command) => {
            Err(anyhow::anyhow!("'{command}' is not implemented yet"))
        }
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
