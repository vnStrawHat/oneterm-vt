//! A grid plus the interner it needs, with helpers that check every invariant after each step.

use crate::cell::{Cell, CellWidth, Style};
use crate::grid::{Pos, PrintMode, RowId, Size, TerminalGrid};
use crate::intern::Interner;
use crate::width::scalar_width;
use crate::{ResizeOutcome, ResizePolicy};

pub(crate) struct Fixture {
    pub grid: TerminalGrid,
    pub interner: Interner,
}

/// A grid of `rows` x `cols` with 100 rows of scrollback.
pub(crate) fn fixture(rows: u16, cols: u16) -> Fixture {
    fixture_with(rows, cols, 100)
}

pub(crate) fn fixture_with(rows: u16, cols: u16, scrollback: u32) -> Fixture {
    Fixture {
        grid: TerminalGrid::new(Size { rows, cols }, scrollback),
        interner: Interner::default(),
    }
}

impl Fixture {
    /// Asserts that every integrity check passes, anchors synced first.
    pub fn check(&mut self) {
        self.grid.sync_anchors();
        assert_eq!(self.grid.check_integrity(Some(&self.interner)), Ok(()));
    }

    pub fn print(&mut self, text: &str) {
        self.print_with(PrintMode::default(), text);
    }

    pub fn print_with(&mut self, mode: PrintMode, text: &str) {
        for c in text.chars() {
            self.grid
                .print(c, scalar_width(c), mode, &mut self.interner);
            self.check();
        }
    }

    /// Carriage return, then line feed.
    pub fn newline(&mut self) {
        self.grid.screen_mut().carriage_return();
        self.grid.linefeed();
        self.check();
    }

    pub fn goto(&mut self, index: u16, col: u16) {
        self.grid.screen_mut().goto(index, col);
        self.check();
    }

    /// Makes the pen use `style`.
    pub fn set_style(&mut self, style: Style) {
        let template = Cell::EMPTY.with_style(self.interner.style(style));
        self.grid
            .screen_mut()
            .set_template(template, &mut self.interner);
        self.check();
    }

    pub fn row_text(&self, id: RowId) -> String {
        let mut text = String::new();
        self.grid
            .screen()
            .row_text(id, &self.interner.graphemes, &mut text);
        text
    }

    pub fn index_text(&self, index: u16) -> String {
        self.row_text(self.grid.screen().row_of_index(index))
    }

    /// Labels every screen row `r0`, `r1`, ...; leaves the cursor on the last row.
    pub fn label_rows(&mut self) {
        for index in 0..self.grid.screen().rows() {
            self.goto(index, 0);
            self.print(&format!("r{index}"));
        }
    }

    /// The screen rows as text, trailing blanks trimmed, joined by `|`.
    pub fn screen_text(&self) -> String {
        (0..self.grid.screen().rows())
            .map(|index| self.index_text(index).trim_end().to_string())
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Every live id of the active screen, oldest first.
    pub fn live_ids(&self) -> Vec<RowId> {
        let screen = self.grid.screen();
        let mut ids = Vec::new();
        let mut id = screen.oldest();
        while id <= screen.newest() {
            ids.push(id);
            id = id + 1;
        }
        ids
    }

    /// Resizes the grid and checks every invariant.
    pub fn resize(&mut self, rows: u16, cols: u16, policy: ResizePolicy) -> ResizeOutcome {
        let outcome = self.grid.resize(Size { rows, cols }, policy);
        self.check();
        outcome
    }

    /// The active screen's logical lines, oldest first: rows joined on `WRAPPED`, spacers
    /// skipped, trailing spaces trimmed.
    pub fn lines(&self) -> Vec<String> {
        let screen = self.grid.screen();
        let mut lines = Vec::new();
        let mut line = String::new();
        for id in self.live_ids() {
            line.push_str(&self.row_text(id));
            if !screen.row(id).wrapped() {
                lines.push(line.trim_end().to_string());
                line.clear();
            }
        }
        if !line.is_empty() {
            lines.push(line.trim_end().to_string());
        }
        lines
    }

    /// A position as (logical line, offset in the line) on the active screen.
    pub fn line_offset(&self, pos: Pos) -> (usize, usize) {
        let screen = self.grid.screen();
        let (mut line, mut offset) = (0, 0);
        for id in self.live_ids().into_iter().take_while(|&id| id < pos.row) {
            let row = screen.row(id);
            if row.wrapped() {
                let filler = row
                    .cells()
                    .last()
                    .is_some_and(|cell| cell.width() == CellWidth::LeadingWideSpacer);
                offset += row.cells().len() - usize::from(filler);
            } else {
                line += 1;
                offset = 0;
            }
        }
        (line, offset + usize::from(pos.col))
    }

    /// The cursor as (logical line, offset in the line, pending wrap).
    pub fn cursor_at(&self) -> (usize, usize, bool) {
        let cursor = self.grid.screen().cursor();
        let (line, offset) = self.line_offset(cursor.pos);
        (line, offset, cursor.pending_wrap)
    }
}
