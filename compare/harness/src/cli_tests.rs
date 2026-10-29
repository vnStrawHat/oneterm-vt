use super::*;
use crate::dump::{Cell, DumpBuilder};
use crate::engine::{Capability, EngineInfo};

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(ToString::to_string).collect()
}

#[test]
fn defaults_match_the_agreed_knobs() {
    let inv = parse(&args(&["grid", "--file", "f.vt"])).unwrap();
    assert_eq!(
        (
            inv.geometry.cols,
            inv.geometry.rows,
            inv.geometry.scrollback
        ),
        (160, 45, 10_000)
    );
    assert_eq!(inv.chunk, 65_536);
    assert_eq!(
        inv.command,
        Command::Grid {
            file: "f.vt".into(),
            passes: 5,
            warmup: 1
        }
    );
    let verify = parse(&args(&["verify", "--file", "f.vt", "--chunk", "100"])).unwrap();
    assert_eq!(verify.chunk, 100);
    assert_eq!(
        verify.command,
        Command::Verify {
            file: "f.vt".into(),
            checkpoints: 4
        }
    );
    assert_eq!(parse(&args(&["info"])).unwrap().command, Command::Info);
}

#[test]
fn bad_arguments_are_usage_errors() {
    for bad in [
        args(&[]),
        args(&["frobnicate"]),
        args(&["grid"]),
        args(&["verify"]),
        args(&["grid", "--file", "f", "--bogus", "1"]),
        args(&["grid", "--file", "f", "--checkpoints", "4"]),
        args(&["verify", "--file", "f", "--passes", "4"]),
        args(&["info", "--file", "f"]),
        args(&["grid", "--file"]),
        args(&["grid", "--file", "f", "--passes", "x"]),
        args(&["grid", "--file", "f", "--cols", "99999"]),
    ] {
        assert!(parse(&bad).is_err(), "{bad:?}");
    }
}

/// Counts bytes, prints nothing itself.
struct Counting(usize);

impl Engine for Counting {
    const INFO: EngineInfo = EngineInfo {
        name: "counting",
        version: "0",
        source: "test",
        language: "rust",
        build: "test",
        capabilities: &[Capability::Hyperlinks],
        labels: &[],
    };
    fn new(_: Geometry) -> Self {
        Self(0)
    }
    fn feed(&mut self, bytes: &[u8]) {
        self.0 += bytes.len();
    }
    fn dump(&self, out: &mut DumpBuilder) {
        out.row(|row| {
            row.cell(Cell {
                text: "x",
                ..Cell::default()
            });
        });
    }
}

#[test]
fn execute_returns_exactly_one_json_object() {
    let dir = std::env::temp_dir().join("compare-harness-cli-test");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("input.vt");
    std::fs::write(&file, vec![b'a'; 300_000]).unwrap();
    let path = file.to_str().unwrap();
    for command in [
        args(&["info"]),
        args(&["grid", "--file", path, "--passes", "2"]),
        args(&["verify", "--file", path]),
    ] {
        let inv = parse(&command).unwrap();
        let out = execute::<Counting>(&inv).unwrap();
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(value.is_object(), "{out}");
        assert!(!out.contains('\n'));
    }
    let grid: serde_json::Value = serde_json::from_str(
        &execute::<Counting>(&parse(&args(&["grid", "--file", path, "--passes", "2"])).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(grid["bytes"], 300_000);
    assert_eq!(grid["samples_ns"].as_array().unwrap().len(), 2);
    let missing = parse(&args(&["grid", "--file", "does-not-exist.vt"])).unwrap();
    assert!(execute::<Counting>(&missing).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
