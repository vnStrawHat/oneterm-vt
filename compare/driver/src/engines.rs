//! The nine engines: fixed order, how each adapter is started, and the process runner
//! (priority, timeout, output capture).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// The fixed engine order of every table, chart and rotation.
pub const ORDER: [&str; 9] = [
    "oneterm",
    "ghostty",
    "alacritty",
    "rio",
    "wezterm",
    "vt100",
    "avt",
    "libvterm",
    "xterm",
];

/// Seconds a single adapter process may run before it counts as failed.
pub const PROCESS_TIMEOUT: Duration = Duration::from_secs(600);

/// Where things live: the repository root, the shared `target/` and `.tmp/compare/`.
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub target: PathBuf,
}

impl Paths {
    /// The repository this driver was built in; `CARGO_TARGET_DIR` wins over `<root>/target`.
    pub fn discover() -> Paths {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(Path::parent)
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let target =
            std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
        Paths { root, target }
    }

    pub fn compare_dir(&self) -> PathBuf {
        self.root.join("compare")
    }

    pub fn work_dir(&self) -> PathBuf {
        self.root.join(".tmp").join("compare")
    }

    pub fn corpus_dir(&self) -> PathBuf {
        self.work_dir().join("corpus")
    }

    pub fn verify_dir(&self) -> PathBuf {
        self.work_dir().join("verify")
    }

    pub fn is_node(name: &str) -> bool {
        name == "xterm"
    }

    /// The command that starts the adapter of `engine` (arguments still to add).
    pub fn command(&self, engine: &str) -> Command {
        if Paths::is_node(engine) {
            let mut command = Command::new("node");
            command
                .arg("--expose-gc")
                .arg(self.compare_dir().join("engines/xterm/adapter.mjs"));
            command
        } else {
            let exe = format!("engine-{engine}{}", std::env::consts::EXE_SUFFIX);
            Command::new(self.target.join("release").join(exe))
        }
    }
}

/// Engine order for repetition `r`: the fixed order rotated left by `r`.
pub fn rotated(names: &[String], repetition: usize) -> Vec<String> {
    if names.is_empty() {
        return Vec::new();
    }
    let start = repetition % names.len();
    names[start..]
        .iter()
        .chain(&names[..start])
        .cloned()
        .collect()
}

/// Whether adapter processes get the Windows `HIGH_PRIORITY_CLASS`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Priority {
    Normal,
    High,
}

/// Runs `command` to completion and returns its stdout. Any failure (spawn, non-zero exit,
/// timeout) is an error whose text starts with the first stderr line.
pub fn run_adapter(mut command: Command, timeout: Duration, priority: Priority) -> Result<String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if priority == Priority::High {
        set_high_priority(&mut command);
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("cannot start {:?}", command.get_program()))?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut text);
            }
            text
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().context("cannot poll the adapter")? {
            break Some(status);
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = String::from_utf8_lossy(&stdout.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&stderr.join().unwrap_or_default()).into_owned();
    let first_line: String = stderr
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect();
    match status {
        None => bail!("timed out after {} s {first_line}", timeout.as_secs()),
        Some(status) if !status.success() => bail!("exit {status}: {first_line}"),
        Some(_) => Ok(stdout),
    }
}

// The one place the child priority is set; the call is behind `cfg(windows)`.
fn set_high_priority(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // HIGH_PRIORITY_CLASS
        command.creation_flags(0x0000_0080);
    }
    #[cfg(not(windows))]
    let _ = command;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        ORDER.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn repetition_r_starts_at_engine_r_mod_nine_and_runs_each_once() {
        for r in 0..20 {
            let order = rotated(&names(), r);
            assert_eq!(order.len(), 9);
            assert_eq!(order[0], ORDER[r % 9]);
            let mut sorted = order.clone();
            sorted.sort();
            let mut expected = names();
            expected.sort();
            assert_eq!(sorted, expected);
        }
        assert!(rotated(&[], 3).is_empty());
    }

    #[test]
    fn a_missing_program_is_an_error_naming_it() {
        let error = run_adapter(
            Command::new("definitely-not-a-program-0016"),
            Duration::from_secs(1),
            Priority::Normal,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("definitely-not-a-program-0016"));
    }
}
