//! What an integrity check can report.

use crate::grid::{AnchorId, RowFlags, RowId, ScreenKind};

/// The first invariant a check found broken.
///
/// Checks return this; production code only `debug_assert!`s the result, so release builds
/// never panic and debug builds stop at the first broken invariant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IntegrityViolation {
    IdsOutOfOrder,
    TooFewLiveRows,
    RingOverflow,
    HistoryOverLimit,
    OffsetPastHistory,
    LaneOverlap,
    CursorOutside { saved: bool },
    RegionInvalid,
    StaleSlot { slot: usize },
    RowIdMismatch { id: RowId },
    RowWidth { id: RowId },
    BrokenWidePair { id: RowId, col: u16 },
    LeadingSpacerInland { id: RowId, col: u16 },
    MissingHint { id: RowId, flag: RowFlags },
    OccTailNotUniform { id: RowId },
    AnchorOutsideLane { id: AnchorId },
    ScreenOwnedAnchors { kind: ScreenKind, live: usize },
    UnknownStyle { id: RowId, col: u16 },
    UnknownExtras { id: RowId, col: u16 },
    UnknownGrapheme { id: RowId, col: u16 },
}
