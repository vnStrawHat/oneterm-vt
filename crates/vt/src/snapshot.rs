//! Damage-tracked snapshots of a terminal, and synchronized output.
//!
//! The model is pull-based. A [`SnapshotState`] belongs to one consumer of one terminal and is
//! reused across frames. Drawing takes two phases: [`Terminal::snapshot_update`] runs under
//! whatever lock the embedder keeps around the terminal and copies only the rows whose change
//! stamp passed the state's [`Watermark`]; then [`SnapshotState::map_colors`] and drawing read
//! the state alone, outside the lock. Nothing is cleared on the terminal's side, so any number
//! of states over one terminal never consume each other's damage.
//!
//! [`Terminal::snapshot_update`]: crate::Terminal::snapshot_update

mod mode_snapshot;
mod palette;
mod row;
mod state;
mod sync;

#[cfg(test)]
mod snapshot_tests;

pub use mode_snapshot::{ModeSnapshot, MouseEncoding, MouseProtocol, MouseReporting};
pub use palette::Palette;
pub use row::{SnapshotCell, SnapshotContent, SnapshotRow, StyleRun};
pub use state::{SnapshotCursor, SnapshotPlacement, SnapshotState, SnapshotUpdate, Watermark};
pub use sync::SyncState;

pub(crate) use state::EngineView;
