//! The adapter contract and the verify pass (spec 0016, harness.md).

use serde::{Deserialize, Serialize};

use crate::dump::{Dump, DumpBuilder};
use crate::memory::Memory;

/// Terminal size and history every engine is created with.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub cols: u16,
    pub rows: u16,
    pub scrollback: usize,
}

/// A protocol feature a fixture needs; an engine that lacks it gets `unsupported`, not a number.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    Sixel,
    Graphemes,
    Hyperlinks,
}

/// What `engine-<name> info` prints.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct EngineInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub source: &'static str,
    pub language: &'static str,
    pub build: &'static str,
    pub capabilities: &'static [Capability],
    /// Notes shown beside the engine name in reports (`+ring`, `+utf8 decode`).
    pub labels: &'static [&'static str],
}

/// `EngineInfo` as the driver reads it back.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InfoRecord {
    pub name: String,
    pub version: String,
    pub source: String,
    pub language: String,
    pub build: String,
    pub capabilities: Vec<String>,
    pub labels: Vec<String>,
}

/// One embeddable terminal core behind the common interface.
pub trait Engine: Sized {
    const INFO: EngineInfo;
    fn new(geometry: Geometry) -> Self;
    fn feed(&mut self, bytes: &[u8]);
    /// Flush deferred work (pending `?2026` buffer, async queue). Timed after the last
    /// feed; in `verify` also called before EVERY checkpoint dump.
    fn finish(&mut self) {}
    fn dump(&self, out: &mut DumpBuilder);
    /// History rows the engine reports holding, for engines whose scrollback limit is set
    /// through a side channel the driver wants proof of (Ghostty, `verify`, fixture `scrolling`).
    fn scrollback_rows(&self) -> Option<u64> {
        None
    }
}

/// Chunk indices (1-based count of chunks fed) of the `count` checkpoints over `chunks`
/// chunks: `ceil(chunks * q / count)` for `q = 1..=count`, at least 1; the last is `chunks`.
pub fn checkpoint_chunks(chunks: usize, count: usize) -> Vec<usize> {
    (1..=count)
        .map(|q| (chunks * q).div_ceil(count).max(1))
        .collect()
}

/// Private bytes at the three moments `verify` samples (criteria.md "Metrics").
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryPoints {
    /// After the corpus file was loaded, before the terminal was created.
    pub base: u64,
    /// After the last feed and `finish`, before the final dump is built.
    pub steady: u64,
    /// Peak private bytes of the process, read after the final dump.
    pub peak: u64,
}

/// `verify` output: one dump per checkpoint, the hash of the last, and memory.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifyOutput {
    pub dumps: Vec<Dump>,
    pub final_sha256: String,
    pub memory: MemoryPoints,
    /// `Engine::scrollback_rows` after the final dump.
    #[serde(default)]
    pub scrollback_rows: Option<u64>,
}

/// Feeds `bytes` in `chunk`-sized slices and dumps at each checkpoint, calling `finish`
/// before every dump. `sample` reads memory: once before the engine exists (`base`), once
/// after the last `finish` and before the final dump allocates (`steady`), once after
/// (`peak`).
pub fn verify_engine<E: Engine>(
    bytes: &[u8],
    geometry: Geometry,
    chunk: usize,
    checkpoints: usize,
    mut sample: impl FnMut() -> Memory,
) -> VerifyOutput {
    let base = sample().private;
    let mut engine = E::new(geometry);
    let chunks: Vec<&[u8]> = bytes.chunks(chunk.max(1)).collect();
    let marks = checkpoint_chunks(chunks.len(), checkpoints);
    let mut dumps = Vec::with_capacity(marks.len());
    let mut fed = 0;
    let mut steady = 0;
    for (index, mark) in marks.iter().enumerate() {
        let upto = (*mark).min(chunks.len());
        for part in &chunks[fed..upto] {
            engine.feed(part);
        }
        fed = upto;
        engine.finish();
        let last = index + 1 == marks.len();
        if last {
            steady = sample().private;
        }
        let mut builder = DumpBuilder::new(geometry.cols, last);
        engine.dump(&mut builder);
        dumps.push(builder.finish());
    }
    let peak = sample().peak;
    let final_sha256 = dumps.last().map(Dump::sha256).unwrap_or_default();
    VerifyOutput {
        scrollback_rows: engine.scrollback_rows(),
        dumps,
        final_sha256,
        memory: MemoryPoints { base, steady, peak },
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod engine_tests;
