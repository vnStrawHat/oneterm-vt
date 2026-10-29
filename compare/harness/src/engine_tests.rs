use std::cell::RefCell;

use super::*;
use crate::dump::Cell;

#[test]
fn checkpoints_are_at_least_one_and_end_on_the_last_chunk() {
    assert_eq!(checkpoint_chunks(1, 4), [1, 1, 1, 1]);
    assert_eq!(checkpoint_chunks(3, 4), [1, 2, 3, 3]);
    assert_eq!(checkpoint_chunks(4, 4), [1, 2, 3, 4]);
    assert_eq!(checkpoint_chunks(513, 4), [129, 257, 385, 513]);
    assert_eq!(checkpoint_chunks(0, 4), [1, 1, 1, 1]);
}

thread_local! {
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn log(entry: impl Into<String>) {
    LOG.with(|log| log.borrow_mut().push(entry.into()));
}

/// Holds every byte back until `finish`, like vte inside a `?2026` window.
#[derive(Default)]
struct Buffering {
    pending: usize,
    applied: usize,
}

impl Engine for Buffering {
    const INFO: EngineInfo = EngineInfo {
        name: "buffering",
        version: "0",
        source: "test",
        language: "rust",
        build: "test",
        capabilities: &[],
        labels: &[],
    };
    fn new(_: Geometry) -> Self {
        log("new");
        Self::default()
    }
    fn feed(&mut self, bytes: &[u8]) {
        self.pending += bytes.len();
    }
    fn finish(&mut self) {
        self.applied += std::mem::take(&mut self.pending);
        log("finish");
    }
    fn dump(&self, out: &mut DumpBuilder) {
        log(format!("dump {}", self.applied));
        out.row(|row| {
            row.cell(Cell {
                text: if self.applied > 0 { "y" } else { "n" },
                ..Cell::default()
            });
        });
    }
}

const GEOMETRY: Geometry = Geometry {
    cols: 10,
    rows: 2,
    scrollback: 100,
};

#[test]
fn finish_runs_before_every_checkpoint_dump() {
    LOG.with(|log| log.borrow_mut().clear());
    let bytes = vec![b'a'; 40];
    let out = verify_engine::<Buffering>(&bytes, GEOMETRY, 10, 4, Memory::default);
    // Without `finish` first, the first dump would show nothing applied.
    assert_eq!(
        LOG.with(|log| log.borrow().clone()),
        [
            "new", "finish", "dump 10", "finish", "dump 20", "finish", "dump 30", "finish",
            "dump 40"
        ]
    );
    assert_eq!(out.dumps.len(), 4);
    assert!(out.dumps.iter().all(|dump| dump.rows == ["y"]));
    assert_eq!(out.final_sha256, out.dumps[3].sha256());
}

#[test]
fn history_is_only_in_the_last_dump() {
    struct WithHistory;
    impl Engine for WithHistory {
        const INFO: EngineInfo = Buffering::INFO;
        fn new(_: Geometry) -> Self {
            Self
        }
        fn feed(&mut self, _: &[u8]) {}
        fn dump(&self, out: &mut DumpBuilder) {
            out.history_row(|row| {
                row.cell(Cell {
                    text: "old",
                    ..Cell::default()
                });
            });
        }
    }
    let out = verify_engine::<WithHistory>(&[0; 8], GEOMETRY, 2, 4, Memory::default);
    assert!(out.dumps[..3].iter().all(|dump| dump.history.is_empty()));
    assert_eq!(out.dumps[3].history, ["old"]);
}

#[test]
fn memory_is_sampled_around_the_terminal_and_before_the_final_dump_builds() {
    LOG.with(|log| log.borrow_mut().clear());
    let mut readings = [1u64, 2, 3].into_iter();
    let out = verify_engine::<Buffering>(&[b'a'; 40], GEOMETRY, 10, 4, || {
        log("sample");
        let value = readings.next().unwrap_or(0);
        Memory {
            private: value * 100,
            peak: value * 1000,
        }
    });
    let events = LOG.with(|log| log.borrow().clone());
    // base before the engine exists, steady after the last finish and before its dump.
    assert_eq!(events[0], "sample");
    assert_eq!(events[1], "new");
    let last_finish = events.iter().rposition(|e| e == "finish").unwrap();
    assert_eq!(events[last_finish + 1], "sample");
    assert_eq!(events[last_finish + 2], "dump 40");
    assert_eq!(
        out.memory,
        MemoryPoints {
            base: 100,
            steady: 200,
            peak: 3000
        }
    );
}
