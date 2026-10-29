//! The adapter command line (harness.md): `info`, `grid`, `verify`, one JSON object on stdout.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::engine::{Engine, Geometry, verify_engine};
use crate::memory;
use crate::timing::run_grid;

/// What one adapter invocation does.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Info,
    Grid {
        file: PathBuf,
        passes: usize,
        warmup: usize,
    },
    Verify {
        file: PathBuf,
        checkpoints: usize,
    },
}

#[derive(Debug)]
pub struct Invocation {
    pub geometry: Geometry,
    pub chunk: usize,
    pub command: Command,
}

fn number(flag: &str, value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .ok()
        .filter(|n| *n > 0 || flag == "--warmup")
        .ok_or_else(|| format!("{flag} needs a whole number, got '{value}'"))
}

/// Parses `args` (without the program name); `Err` is a usage message and means exit 2.
pub fn parse(args: &[String]) -> Result<Invocation, String> {
    let (command, flags) = args
        .split_first()
        .ok_or("missing command: info | grid | verify")?;
    if !matches!(command.as_str(), "info" | "grid" | "verify") {
        return Err(format!("unknown command '{command}'"));
    }
    let mut geometry = Geometry {
        cols: 160,
        rows: 45,
        scrollback: 10_000,
    };
    let (mut chunk, mut passes, mut warmup, mut checkpoints) = (65_536, 5, 1, 4);
    let mut file = None;
    let mut iter = flags.iter();
    while let Some(flag) = iter.next() {
        let allowed = match flag.as_str() {
            "--cols" | "--rows" | "--scrollback" | "--chunk" => true,
            "--file" => command != "info",
            "--passes" | "--warmup" => command == "grid",
            "--checkpoints" => command == "verify",
            _ => false,
        };
        if !allowed {
            return Err(format!("unknown flag '{flag}' for '{command}'"));
        }
        let value = iter
            .next()
            .ok_or_else(|| format!("flag '{flag}' needs a value"))?;
        match flag.as_str() {
            "--file" => file = Some(PathBuf::from(value)),
            "--cols" => {
                geometry.cols = u16::try_from(number(flag, value)?).map_err(|e| e.to_string())?
            }
            "--rows" => {
                geometry.rows = u16::try_from(number(flag, value)?).map_err(|e| e.to_string())?
            }
            "--scrollback" => geometry.scrollback = number(flag, value)?,
            "--chunk" => chunk = number(flag, value)?,
            "--passes" => passes = number(flag, value)?,
            "--warmup" => warmup = number(flag, value)?,
            _ => checkpoints = number(flag, value)?,
        }
    }
    let command = match command.as_str() {
        "info" => Command::Info,
        "grid" => Command::Grid {
            file: file.ok_or("grid needs --file F")?,
            passes,
            warmup,
        },
        _ => Command::Verify {
            file: file.ok_or("verify needs --file F")?,
            checkpoints,
        },
    };
    Ok(Invocation {
        geometry,
        chunk,
        command,
    })
}

/// Runs the invocation and returns the JSON object for stdout; `Err` means exit 1.
pub fn execute<E: Engine>(invocation: &Invocation) -> Result<String, String> {
    let Invocation {
        geometry,
        chunk,
        command,
    } = invocation;
    let json = |result: Result<String, serde_json::Error>| result.map_err(|e| e.to_string());
    match command {
        Command::Info => json(serde_json::to_string(&E::INFO)),
        Command::Grid {
            file,
            passes,
            warmup,
        } => {
            let bytes = read(file)?;
            json(serde_json::to_string(&run_grid::<E>(
                &bytes, *geometry, *chunk, *passes, *warmup,
            )))
        }
        Command::Verify { file, checkpoints } => {
            let bytes = read(file)?;
            json(serde_json::to_string(&verify_engine::<E>(
                &bytes,
                *geometry,
                *chunk,
                *checkpoints,
                memory::sample,
            )))
        }
    }
}

fn read(file: &PathBuf) -> Result<Vec<u8>, String> {
    std::fs::read(file).map_err(|e| format!("cannot read {}: {e}", file.display()))
}

/// The `main` of every Rust adapter: `fn main() -> ExitCode { compare_harness::main::<Mine>() }`.
// The adapter binary is the one place that talks to stdout and stderr.
#[allow(clippy::print_stdout)]
pub fn main<E: Engine>() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let name = E::INFO.name;
    let invocation = match parse(&args) {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("engine-{name}: {message}");
            return ExitCode::from(2);
        }
    };
    match execute::<E>(&invocation) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("engine-{name}: {message}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod cli_tests;
