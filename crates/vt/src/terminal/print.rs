//! Printing text: the scalar path, the `?2027` cluster path, charset mapping and REP.

use unicode_segmentation::UnicodeSegmentation;

use crate::grid::{Charset, PrintMode, RowId};
use crate::width::{cluster_width, scalar_width};

use super::dispatch::Handler;
use super::modes::Mode;

/// Longest cluster, in scalars, that a following chunk may still extend.
pub(super) const CLUSTER_CARRY_MAX: usize = 32;

/// The cell of the last cluster `?2027` placed, so text arriving in the next chunk can extend
/// it. The cluster's text is `State::carry_text`.
#[derive(Clone, Copy, Debug)]
pub(super) struct ClusterCarry {
    row: RowId,
    col: u16,
    /// The GL set the cluster was printed with.
    set: u8,
}

/// Maps a scalar through a designated set. Only the DEC line-drawing set changes anything.
pub(super) fn map_charset(charset: Charset, c: char) -> char {
    if charset != Charset::SpecialCharacterAndLineDrawing {
        return c;
    }
    match c {
        '_' => ' ',
        '`' => '\u{25C6}',
        'a' => '\u{2592}',
        'b' => '\u{2409}',
        'c' => '\u{240C}',
        'd' => '\u{240D}',
        'e' => '\u{240A}',
        'f' => '\u{00B0}',
        'g' => '\u{00B1}',
        'h' => '\u{2424}',
        'i' => '\u{240B}',
        'j' => '\u{2518}',
        'k' => '\u{2510}',
        'l' => '\u{250C}',
        'm' => '\u{2514}',
        'n' => '\u{253C}',
        'o' => '\u{23BA}',
        'p' => '\u{23BB}',
        'q' => '\u{2500}',
        'r' => '\u{23BC}',
        's' => '\u{23BD}',
        't' => '\u{251C}',
        'u' => '\u{2524}',
        'v' => '\u{2534}',
        'w' => '\u{252C}',
        'x' => '\u{2502}',
        'y' => '\u{2264}',
        'z' => '\u{2265}',
        '{' => '\u{03C0}',
        '|' => '\u{2260}',
        '}' => '\u{00A3}',
        '~' => '\u{00B7}',
        other => other,
    }
}

impl Handler<'_> {
    /// A run of printable text from the parser.
    pub(super) fn print_text(&mut self, text: &str) {
        self.state.dispatched = true;
        if self.state.modes.get(Mode::GraphemeClusters) {
            self.print_clusters(text);
        } else {
            self.print_scalars(text);
        }
    }

    /// The set the next character uses: a pending single shift once, else the GL set.
    fn take_set(&mut self) -> u8 {
        self.state
            .shifts
            .single
            .take()
            .unwrap_or(self.state.shifts.active)
    }

    /// One scalar through `charset`: maps, measures and prints it. Returns its width.
    fn print_mapped(&mut self, c: char, charset: Charset, mode: PrintMode) -> u8 {
        let mapped = map_charset(charset, c);
        let width = scalar_width(mapped);
        let report = self
            .state
            .grid
            .print(mapped, width, mode, &mut self.state.interner);
        self.report(report);
        width
    }

    /// One scalar through the scalar path, taking its set as the text path does.
    fn print_scalar(&mut self, c: char) {
        let mode = self.state.print_mode();
        let set = self.take_set();
        let charset = self.state.charset(set);
        self.print_mapped(c, charset, mode);
        self.state.preceding_char = Some(c);
    }

    /// The default path: every scalar on its own, measured alone. Runs of narrow scalars go
    /// through one row borrow; anything else takes the general per-scalar path.
    fn print_scalars(&mut self, text: &str) {
        let mode = self.state.print_mode();
        let mut text = text;
        if self.state.shifts.single.is_some()
            && let Some(first) = text.chars().next()
        {
            self.print_scalar(first);
            text = &text[first.len_utf8()..];
        }
        // Nothing in a run changes the designations, so the set is looked up once.
        let charset = self.state.charset(self.state.shifts.active);
        let narrow = |c: char| {
            let mapped = map_charset(charset, c);
            (!mapped.is_control() && scalar_width(mapped) == 1).then_some(mapped)
        };
        let last = text.chars().next_back();
        // After a scalar that was not one column wide the next is likely the same (CJK text),
        // so the run is not tried again until a narrow one goes through the general path.
        let mut try_run = true;
        while !text.is_empty() {
            if try_run {
                let run = self.state.grid.print_narrow_run(text, mode, narrow);
                text = &text[run..];
            }
            let Some(c) = text.chars().next() else {
                break;
            };
            try_run = self.print_mapped(c, charset, mode) == 1;
            text = &text[c.len_utf8()..];
        }
        if last.is_some() {
            self.state.preceding_char = last;
        }
    }

    /// `?2027`: whole grapheme clusters, one cell each. A cluster that ends a chunk may be
    /// extended by the next one, so the last cluster is remembered with its cell.
    fn print_clusters(&mut self, text: &str) {
        let mode = self.state.print_mode();
        let mut joined = std::mem::take(&mut self.state.scratch);
        joined.clear();
        let carry = self.state.carry.take();
        if carry.is_some() {
            joined.push_str(&self.state.carry_text);
        }
        let head = joined.len();
        joined.push_str(text);
        let mut skip = 0;
        if let Some(carry) = carry
            && let Some(first) = joined.graphemes(true).next()
        {
            if first.len() > head {
                // The run extends the carried cluster: print it again over its own cell.
                let screen = self.state.grid.screen_mut();
                if let Some(index) = screen.index_of(carry.row) {
                    screen.goto(index, carry.col);
                }
                // The cell already exists: IRM must not shift the row to make room again.
                self.place_cluster(
                    first,
                    carry.set,
                    PrintMode {
                        insert: false,
                        ..mode
                    },
                );
            }
            skip = first.len().max(head);
        }
        for cluster in joined.get(skip..).unwrap_or_default().graphemes(true) {
            let set = self.take_set();
            self.place_cluster(cluster, set, mode);
        }
        self.state.scratch = joined;
    }

    /// Prints one cluster: its first scalar with the cluster's width, the rest attached.
    fn place_cluster(&mut self, cluster: &str, set: u8, mode: PrintMode) {
        let width = cluster_width(cluster);
        let charset = self.state.charset(set);
        let mut scalars = 0usize;
        for (index, c) in cluster.chars().enumerate() {
            let (mapped, width) = if index == 0 {
                (map_charset(charset, c), width)
            } else {
                (c, 0)
            };
            let report = self
                .state
                .grid
                .print(mapped, width, mode, &mut self.state.interner);
            self.report(report);
            // REP repeats the scalar as received, not as the charset drew it.
            self.state.preceding_char = Some(c);
            scalars += 1;
        }
        if width == 0 {
            self.state.carry = None;
        } else if scalars > CLUSTER_CARRY_MAX {
            self.state.carry = None;
            let stats = &mut self.state.stats;
            stats.dropped_cluster_carries = stats.dropped_cluster_carries.saturating_add(1);
        } else {
            let cursor = self.state.grid.screen().cursor();
            let end = if cursor.pending_wrap {
                self.state.grid.screen().cols()
            } else {
                cursor.pos.col
            };
            self.state.carry = Some(ClusterCarry {
                row: cursor.pos.row,
                col: end.saturating_sub(u16::from(width)),
                set,
            });
            self.state.carry_text.clear();
            self.state.carry_text.push_str(cluster);
        }
    }

    /// REP: prints the last printed scalar `n` more times through the scalar path.
    pub(super) fn repeat_preceding(&mut self, n: u16) {
        let Some(c) = self.state.preceding_char else {
            return;
        };
        for _ in 0..n {
            self.print_scalar(c);
        }
    }
}
