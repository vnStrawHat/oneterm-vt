//! Adapter side of the cross-engine comparison (docs/specs/0016-cross-engine-bench): the
//! [`Engine`] trait, the adapter command line, the timed pass, the canonical dump and memory.

mod cli;
mod dump;
mod engine;
mod memory;
mod timing;

pub use cli::{Command, Invocation, execute, main, parse};
pub use dump::{
    AttrRun, Cell, Color, Dump, DumpBuilder, Flags, LinkRun, RowCells, hex, sha256_hex,
};
pub use engine::{
    Capability, Engine, EngineInfo, Geometry, InfoRecord, MemoryPoints, VerifyOutput,
    checkpoint_chunks, verify_engine,
};
pub use memory::{Memory, sample as sample_memory};
pub use timing::{GridOutput, Stats, mib_per_second, run_grid, summarize};
