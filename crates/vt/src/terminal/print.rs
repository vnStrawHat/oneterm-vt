//! Printing text: the scalar path, the `?2027` cluster path, charset mapping and REP.

use unicode_segmentation::UnicodeSegmentation;

use crate::grid::{Charset, PrintMode, RowId};
use crate::width::{cluster_str_width, cluster_str_width_cjk, scalar_width, scalar_width_cjk};

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

/// The columns of one scalar: ambiguous-width ones are 2 when `ambiguous_wide`.
#[inline]
fn measure(c: char, ambiguous_wide: bool) -> u8 {
    if ambiguous_wide {
        scalar_width_cjk(c)
    } else {
        scalar_width(c)
    }
}

/// What a print run needs of a scalar under `charset`: the mapped scalar and its width, or
/// `None` for a width-0 scalar or a control.
fn mapped_glyph(charset: Charset) -> impl Fn(char) -> Option<(char, u8)> + Copy {
    move |c| {
        let mapped = map_charset(charset, c);
        let width = scalar_width(mapped);
        (!mapped.is_control() && width >= 1).then_some((mapped, width))
    }
}

/// [`mapped_glyph`] with ambiguous-width scalars two columns wide. A closure of its own, so
/// the print run is compiled once per width rule and the choice costs nothing per glyph
/// (measured: a flag read inside one closure cost `cjk_wide` 9 %).
fn mapped_glyph_cjk(charset: Charset) -> impl Fn(char) -> Option<(char, u8)> + Copy {
    move |c| {
        let mapped = map_charset(charset, c);
        let width = scalar_width_cjk(mapped);
        (!mapped.is_control() && width >= 1).then_some((mapped, width))
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

    /// The grid's print run under the terminal's width rule; returns the bytes it consumed.
    fn print_run(
        &mut self,
        text: &str,
        mode: PrintMode,
        charset: Charset,
        ambiguous_wide: bool,
    ) -> usize {
        let grid = &mut self.state.grid;
        if ambiguous_wide {
            grid.print_run(text, mode, charset, mapped_glyph_cjk(charset))
        } else {
            grid.print_run(text, mode, charset, mapped_glyph(charset))
        }
    }

    /// One scalar through `charset`: maps, measures and prints it. Returns its width.
    fn print_mapped(&mut self, c: char, charset: Charset, mode: PrintMode) -> u8 {
        let mapped = map_charset(charset, c);
        let width = measure(mapped, self.state.config.ambiguous_wide);
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
        let ambiguous_wide = self.state.config.ambiguous_wide;
        // While `?69` is set every glyph takes the general path, which knows the margins.
        let bulk = !self.state.modes.get(Mode::LeftRightMargins);
        let last = text.chars().next_back();
        let is_mark = |c: char| !c.is_control() && scalar_width(map_charset(charset, c)) == 0;
        let mut try_run = bulk;
        while !text.is_empty() {
            if try_run {
                let run = self.print_run(text, mode, charset, ambiguous_wide);
                text = &text[run..];
            }
            let Some(c) = text.chars().next() else {
                break;
            };
            if is_mark(c) {
                // A run of marks joins the glyph before it with one intern, not one per mark.
                let len = text.find(|c: char| !is_mark(c)).unwrap_or(text.len());
                let (marks, rest) = text.split_at(len);
                self.state
                    .grid
                    .attach_marks(marks, &mut self.state.interner);
                text = rest;
                try_run = bulk;
                continue;
            }
            let width = self.print_mapped(c, charset, mode);
            try_run = bulk && width != 0;
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
        let mut rest = joined.get(skip..).unwrap_or_default();
        while !rest.is_empty() {
            // Two printable ASCII scalars never share a cluster, so all but the last of a leading
            // ASCII word go through the print run. The last may still be extended by what follows
            // (here or in the next chunk), so it takes the cluster path.
            if self.state.shifts.single.is_none() {
                let ascii = rest
                    .bytes()
                    .take_while(|byte| (0x20..=0x7E).contains(byte))
                    .count();
                if ascii >= 2 {
                    let (word, tail) = rest.split_at(ascii - 1);
                    self.print_ascii_word(word, mode);
                    rest = tail;
                    continue;
                }
            }
            let Some(cluster) = rest.graphemes(true).next() else {
                break;
            };
            let set = self.take_set();
            self.place_cluster(cluster, set, mode);
            rest = rest.get(cluster.len()..).unwrap_or_default();
        }
        self.state.scratch = joined;
    }

    /// Prints printable ASCII `word` as one cluster per letter, in bulk where the print run can.
    /// Where it cannot (insert mode, a pending wrap, a wide-pair half at the start) one letter
    /// takes the cluster path.
    fn print_ascii_word(&mut self, word: &str, mode: PrintMode) {
        let set = self.state.shifts.active;
        let charset = self.state.charset(set);
        let ambiguous_wide = self.state.config.ambiguous_wide;
        let bulk = !self.state.modes.get(Mode::LeftRightMargins);
        let mut rest = word;
        while !rest.is_empty() {
            let run = if bulk {
                self.print_run(rest, mode, charset, ambiguous_wide)
            } else {
                0
            };
            if run > 0 {
                rest = rest.get(run..).unwrap_or_default();
                continue;
            }
            let (letter, tail) = rest.split_at(1);
            self.place_cluster(letter, set, mode);
            rest = tail;
        }
    }

    /// Prints one cluster: its first scalar with the cluster's width, the rest attached.
    fn place_cluster(&mut self, cluster: &str, set: u8, mode: PrintMode) {
        let width = if self.state.config.ambiguous_wide {
            cluster_str_width_cjk(cluster)
        } else {
            cluster_str_width(cluster)
        };
        let charset = self.state.charset(set);
        let mut scalars = cluster.chars();
        let first = scalars.next();
        let marks = scalars.as_str();
        if let Some(first) = first {
            if width > 0 {
                let report = self.state.grid.print(
                    map_charset(charset, first),
                    width,
                    mode,
                    &mut self.state.interner,
                );
                self.report(report);
            }
            // A cluster of width 0 is all marks; a control never starts one the parser lets by.
            let attached = if width > 0 { marks } else { cluster };
            if !attached.is_empty() && !first.is_control() {
                self.state
                    .grid
                    .attach_marks(attached, &mut self.state.interner);
            }
        }
        // REP repeats the scalar as received, not as the charset drew it.
        self.state.preceding_char = cluster.chars().next_back();
        let scalars = cluster.chars().count();
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

    /// REP: prints the last printed scalar `n` more times, as if the copies had been sent.
    pub(super) fn repeat_preceding(&mut self, n: u16) {
        let Some(c) = self.state.preceding_char else {
            return;
        };
        // A glyph of width 1 or 2 repeats through the text path in runs; a pending single
        // shift applies to the first copy only, as it would to the first of the copies sent.
        if scalar_width(c) > 0 {
            let mut buffer = [0u8; 256];
            let width = c.len_utf8();
            let fit = buffer.len() / width;
            for slot in buffer.chunks_exact_mut(width) {
                c.encode_utf8(slot);
            }
            let mut left = usize::from(n);
            while left > 0 {
                let take = left.min(fit);
                if let Ok(text) = str::from_utf8(buffer.get(..take * width).unwrap_or_default()) {
                    self.print_scalars(text);
                }
                left -= take;
            }
            return;
        }
        for _ in 0..n {
            self.print_scalar(c);
        }
    }
}
