//! `compare`: the cross-engine comparison driver (docs/specs/0016-cross-engine-bench).
//! The binary is the one place that talks to stdout and stderr.
#![allow(clippy::print_stdout)]

mod build;
mod chart;
mod corpus;
mod engines;
mod golden;
mod machine;
mod report;
mod results;
mod run;
mod verify;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

use crate::engines::Paths;
use crate::run::{DEFAULT_CPU, Pin, RunOptions};
use crate::verify::Selection;

const USAGE: &str = "\
compare <command> [flags]

  corpus [--mib 32] [--update-lock]   build vt-bench, write the 23 fixtures, check corpus.lock
  build                               build the eight Rust adapters, npm ci for xterm
  verify [--engine E] [--fixture F] [--reuse]
                                      verify processes, print the status of every pair
                                      (--reuse: print from the stored results, run nothing)
  golden [--fixture F] [--replace]    goldens by plurality, ungated.toml for the rest
  run --machine NAME [--engine E] [--fixture F] [--repetitions 3]
      [--keep-power-plan] [--cpu N] [--no-pin]
                                      verify, time, write perf/compare/NAME/<date>.{json,md,svg}
  report --in FILE                    regenerate the .md and .svg of a results JSON
";

/// `--flag value` and `--switch` arguments of one command; anything left over is an error.
struct Flags {
    args: Vec<String>,
}

impl Flags {
    fn new(args: &[String]) -> Self {
        Self {
            args: args.to_vec(),
        }
    }

    fn switch(&mut self, name: &str) -> bool {
        let before = self.args.len();
        self.args.retain(|arg| arg != name);
        self.args.len() != before
    }

    fn value(&mut self, name: &str) -> Result<Option<String>> {
        let Some(at) = self.args.iter().position(|arg| arg == name) else {
            return Ok(None);
        };
        if at + 1 >= self.args.len() {
            bail!("flag '{name}' needs a value");
        }
        let value = self.args.remove(at + 1);
        self.args.remove(at);
        Ok(Some(value))
    }

    fn number<T: std::str::FromStr>(&mut self, name: &str) -> Result<Option<T>> {
        self.value(name)?
            .map(|text| {
                text.parse().map_err(|_| {
                    anyhow::anyhow!("flag '{name}' needs a whole number, got '{text}'")
                })
            })
            .transpose()
    }

    fn done(self) -> Result<()> {
        match self.args.first() {
            Some(extra) => bail!("unknown argument '{extra}'\n\n{USAGE}"),
            None => Ok(()),
        }
    }
}

fn dispatch(args: &[String]) -> Result<()> {
    let (command, rest) = args.split_first().context(USAGE)?;
    let paths = Paths::discover();
    let mut flags = Flags::new(rest);
    match command.as_str() {
        "corpus" => {
            let mib = flags.number("--mib")?.unwrap_or(corpus::DEFAULT_MIB);
            let update_lock = flags.switch("--update-lock");
            flags.done()?;
            corpus::corpus_command(&paths, mib, update_lock)
        }
        "build" => {
            flags.done()?;
            build::build_command(&paths)
        }
        "verify" => {
            let engine = flags.value("--engine")?;
            let fixture = flags.value("--fixture")?;
            let reuse = flags.switch("--reuse");
            flags.done()?;
            let selection = Selection::new(engine.as_deref(), fixture.as_deref())?;
            verify::verify_command(&paths, &selection, reuse)
        }
        "golden" => {
            let fixture = flags.value("--fixture")?;
            let replace = flags.switch("--replace");
            flags.done()?;
            golden::golden_command(&paths, fixture.as_deref(), replace)
        }
        "run" => {
            let machine = flags
                .value("--machine")?
                .context("run needs --machine NAME")?;
            let engine = flags.value("--engine")?;
            let fixture = flags.value("--fixture")?;
            let repetitions = flags.number("--repetitions")?.unwrap_or(3);
            let cpu = flags.number("--cpu")?;
            let no_pin = flags.switch("--no-pin");
            let keep_power_plan = flags.switch("--keep-power-plan");
            flags.done()?;
            if repetitions == 0 {
                bail!("--repetitions needs at least 1");
            }
            let pin = if no_pin {
                Pin::Off
            } else {
                Pin::Cpu(cpu.unwrap_or(DEFAULT_CPU))
            };
            let written = run::run_command(
                &paths,
                &RunOptions {
                    machine,
                    engine,
                    fixture,
                    repetitions,
                    keep_power_plan,
                    pin,
                },
            )?;
            println!("wrote {}", written.display());
            Ok(())
        }
        "report" => {
            let input = flags.value("--in")?.context("report needs --in FILE")?;
            flags.done()?;
            run::report_command(&PathBuf::from(input))
        }
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        other => bail!("unknown command '{other}'\n\n{USAGE}"),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("compare: {error:#}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(parts: &[&str]) -> Flags {
        Flags::new(&parts.iter().map(ToString::to_string).collect::<Vec<_>>())
    }

    #[test]
    fn flags_take_values_switches_and_reject_leftovers() {
        let mut f = flags(&["--machine", "box", "--no-pin", "--repetitions", "2"]);
        assert_eq!(f.value("--machine").unwrap().as_deref(), Some("box"));
        assert!(f.switch("--no-pin"));
        assert!(!f.switch("--keep-power-plan"));
        assert_eq!(f.number::<usize>("--repetitions").unwrap(), Some(2));
        f.done().unwrap();
        assert!(flags(&["--bogus"]).done().is_err());
        assert!(flags(&["--machine"]).value("--machine").is_err());
        assert!(flags(&["--mib", "x"]).number::<usize>("--mib").is_err());
    }

    #[test]
    fn unknown_commands_are_refused_with_the_usage() {
        let error = dispatch(&["frobnicate".to_string()]).unwrap_err();
        assert!(format!("{error:#}").contains("compare <command>"));
        assert!(dispatch(&[]).is_err());
    }
}
