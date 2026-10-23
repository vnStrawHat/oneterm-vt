//! Positions that follow content.
//!
//! A [`RowId`] names a position, and content moves between positions (region scrolls, later
//! reflow). A consumer that must follow content registers an anchor and reads it back; every
//! row-moving primitive updates the list. The list is small (three per screen plus selection
//! ends, marks and placements), so a linear scan per scroll beats any index.
//!
//! The cursor, saved cursor and viewport-top anchors belong to their screen: it writes them
//! back at the end of every operation that takes the list.

use std::ops::Range;

use crate::grid::{Pos, RowId};
use crate::intern::GraphicId;

/// The handle of one anchor. Only the list issues ids; a released id may be reissued.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AnchorId(u32);

/// What an anchor marks.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AnchorKind {
    /// A screen's cursor (screen-owned).
    Cursor,
    /// A screen's saved cursor (screen-owned).
    SavedCursor,
    /// The first visible row of a screen (screen-owned).
    ViewportTop,
    /// The start of a selection.
    SelectionStart,
    /// The end of a selection.
    SelectionEnd,
    /// A shell-integration prompt mark, tagged by the dispatcher.
    Mark(u32),
    /// One image placement.
    Graphic(GraphicId),
}

impl AnchorKind {
    /// Whether a screen writes this kind back itself.
    pub(crate) fn is_screen_owned(self) -> bool {
        matches!(
            self,
            AnchorKind::Cursor | AnchorKind::SavedCursor | AnchorKind::ViewportTop
        )
    }
}

/// One tracked position.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Anchor {
    /// What it marks.
    pub kind: AnchorKind,
    /// Where its content is now; meaningless once dead.
    pub pos: Pos,
    /// False once its content was scrolled out, trimmed or wiped.
    pub alive: bool,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    anchor: Anchor,
    /// Released entries sit in `free`; a killed entry does not until its owner releases it.
    released: bool,
}

/// The list of anchors of one terminal.
#[derive(Debug, Default)]
pub struct Anchors {
    entries: Vec<Entry>,
    free: Vec<AnchorId>,
}

impl Anchors {
    /// Creates an empty list.
    pub fn new() -> Anchors {
        Anchors::default()
    }

    fn entry(&self, id: AnchorId) -> Option<&Entry> {
        self.entries.get(id.0 as usize)
    }

    /// Returns the position of a live anchor; `None` if it is dead or was never issued.
    pub fn get(&self, id: AnchorId) -> Option<Pos> {
        self.entry(id)
            .filter(|entry| entry.anchor.alive)
            .map(|entry| entry.anchor.pos)
    }

    /// Returns the kind of an issued id, dead or alive.
    pub fn kind(&self, id: AnchorId) -> Option<AnchorKind> {
        self.entry(id).map(|entry| entry.anchor.kind)
    }

    /// Iterates every entry, dead ones included.
    pub fn iter(&self) -> impl Iterator<Item = (AnchorId, &Anchor)> {
        self.entries
            .iter()
            .enumerate()
            // Indices came from `u32` ids.
            .map(|(index, entry)| (AnchorId(index as u32), &entry.anchor))
    }

    /// Returns the number of live anchors.
    pub fn live(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.anchor.alive)
            .count()
    }

    /// Returns the number of live anchors whose row lies in `lane`.
    pub fn live_in(&self, lane: Range<RowId>) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.anchor.alive && lane.contains(&entry.anchor.pos.row))
            .count()
    }

    /// Adds a live anchor, reusing a released slot when there is one.
    pub(crate) fn register(&mut self, kind: AnchorKind, pos: Pos) -> AnchorId {
        let fresh = Entry {
            anchor: Anchor {
                kind,
                pos,
                alive: true,
            },
            released: false,
        };
        if let Some(id) = self.free.pop() {
            if let Some(entry) = self.entries.get_mut(id.0 as usize) {
                *entry = fresh;
            }
            return id;
        }
        // Past `u32::MAX` entries the id is one that `get` never finds.
        let Ok(index) = u32::try_from(self.entries.len()) else {
            return AnchorId(u32::MAX);
        };
        self.entries.push(fresh);
        AnchorId(index)
    }

    /// Moves an anchor and revives it. A released id is left alone.
    pub(crate) fn set(&mut self, id: AnchorId, pos: Pos) {
        if let Some(entry) = self.entries.get_mut(id.0 as usize)
            && !entry.released
        {
            entry.anchor.pos = pos;
            entry.anchor.alive = true;
        }
    }

    /// Frees an anchor's slot. The owner must not use the id afterwards.
    pub(crate) fn release(&mut self, id: AnchorId) {
        if let Some(entry) = self.entries.get_mut(id.0 as usize)
            && !entry.released
        {
            entry.released = true;
            entry.anchor.alive = false;
            self.free.push(id);
        }
    }

    /// Kills anchors in `kill`, then moves the rest of those in `rows` by `delta`.
    ///
    /// Screen-owned anchors are skipped: their screen writes them back.
    pub(crate) fn shift_region(&mut self, rows: Range<RowId>, delta: i32, kill: Range<RowId>) {
        let step = u64::from(delta.unsigned_abs());
        for entry in &mut self.entries {
            let anchor = &mut entry.anchor;
            if !anchor.alive || anchor.kind.is_screen_owned() {
                continue;
            }
            if kill.contains(&anchor.pos.row) {
                anchor.alive = false;
            } else if rows.contains(&anchor.pos.row) {
                anchor.pos.row = if delta >= 0 {
                    anchor.pos.row + step
                } else {
                    anchor.pos.row - step
                };
            }
        }
    }

    /// Kills live anchors with a row in `origin..oldest`: the part of one lane that left the ring.
    ///
    /// Lane-scoped on purpose: a lane-blind "below oldest" would kill the other lane.
    pub(crate) fn trim(&mut self, origin: RowId, oldest: RowId) {
        for entry in &mut self.entries {
            if entry.anchor.alive && (origin..oldest).contains(&entry.anchor.pos.row) {
                entry.anchor.alive = false;
            }
        }
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.entries.capacity() * size_of::<Entry>() + self.free.capacity() * size_of::<AnchorId>()
    }
}

#[cfg(test)]
#[path = "anchor_tests.rs"]
mod anchor_tests;
