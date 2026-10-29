//! `compare build`: every Rust adapter with `cargo build --release` in its own directory, one
//! after another (never in parallel), and `npm ci` for the xterm adapter.

use std::process::Command;

use anyhow::{Result, bail};

use crate::engines::{ORDER, Paths};

fn npm_ci(paths: &Paths) -> Command {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/c", "npm"]);
        command
    } else {
        Command::new("npm")
    };
    command
        .arg("ci")
        .current_dir(paths.compare_dir().join("engines/xterm"))
        .env("npm_config_cache", paths.root.join(".cargo-home/npm"));
    command
}

pub fn build_command(paths: &Paths) -> Result<()> {
    let mut failed = Vec::new();
    for engine in ORDER {
        eprintln!("build {engine}");
        let mut command = if Paths::is_node(engine) {
            npm_ci(paths)
        } else {
            let mut command = Command::new("cargo");
            command
                .args(["build", "--release"])
                .current_dir(paths.compare_dir().join("engines").join(engine));
            command
        };
        match command.status() {
            Ok(status) if status.success() => {}
            Ok(status) => failed.push(format!("{engine} ({status})")),
            Err(error) => failed.push(format!("{engine} ({error})")),
        }
    }
    if !failed.is_empty() {
        bail!("build failed for: {}", failed.join(", "));
    }
    for engine in ORDER {
        let info = crate::verify::read_info(paths, engine)?;
        println!("{:<10} {} ({})", info.name, info.version, info.source);
    }
    Ok(())
}
