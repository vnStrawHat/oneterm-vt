//! A grid plus the interner it needs, with helpers that check every invariant after each step.

use crate::cell::{Cell, Style};
use crate::grid::{PrintMode, RowId, Size, TerminalGrid};
use crate::intern::Interner;
use crate::width::scalar_width;

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
}
