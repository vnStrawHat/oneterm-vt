//! The parser's sink: routes every call to the module that does the work, and handles C0
//! controls, ESC, DCS and APC itself.

use std::time::Instant;

use crate::cell::{Cell, CellContent};
use crate::events::{EventBatch, VtEvent};
use crate::graphics::sixel::SixelDecoder;
use crate::grid::{Charset, ScrollReport};
use crate::parser::{Dispatch, OscParams, Params, StringTerm};
use crate::selection::Invalidation;

use super::State;
use super::modes::Mode;
use super::query::{DcsQuery, QUERY_MAX_BYTES};

/// Applies parser calls to the terminal state and appends events to the batch.
pub(super) struct Handler<'a> {
    pub(super) state: &'a mut State,
    pub(super) out: &'a mut EventBatch,
    /// The time of the feed, for synchronized output deadlines.
    pub(super) now: Instant,
}

impl Handler<'_> {
    /// Counts a sequence that was parsed but not acted on.
    pub(super) fn unhandled(&mut self) {
        let stats = &mut self.state.stats;
        stats.unhandled_sequences = stats.unhandled_sequences.saturating_add(1);
    }

    /// Counts and announces the scroll a grid call ran. Returns the rows that entered history.
    pub(super) fn report(&mut self, report: Option<ScrollReport>) -> u16 {
        let Some(report) = report else {
            return 0;
        };
        let stats = &mut self.state.stats;
        stats.rows_scrolled = stats
            .rows_scrolled
            .saturating_add(u32::from(report.history_rows));
        self.out.push_scroll_report(&report);
        report.history_rows
    }

    /// Everything but printing: counts as a dispatch and breaks a pending `?2027` cluster.
    fn begin_control(&mut self) {
        self.state.dispatched = true;
        self.state.carry = None;
    }

    /// A line feed on either screen: scrolls at the region bottom and counts one line.
    pub(super) fn line_feed(&mut self) {
        self.state.lines_produced = self.state.lines_produced.saturating_add(1);
        let report = self.state.grid.linefeed();
        self.report(report);
    }

    /// C0 controls and C1 bytes.
    fn control(&mut self, byte: u8) {
        match byte {
            0x07 => self.out.push(VtEvent::Bell),
            0x08 => {
                let mode = self.state.print_mode();
                self.state.grid.screen_mut().backspace(mode);
            }
            0x09 => {
                let mode = self.state.print_mode();
                let report = self.state.grid.put_tab(1, mode);
                self.report(report);
            }
            0x0A..=0x0C => self.line_feed(),
            0x0D => self.state.grid.screen_mut().carriage_return(),
            0x0E => self.state.shifts.active = 1,
            0x0F => self.state.shifts.active = 0,
            0x7F => {}
            _ => self.unhandled(),
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if ignore {
            self.unhandled();
            return;
        }
        match (intermediates, byte) {
            ([slot @ (b'(' | b')' | b'*' | b'+')], set) => {
                let slot = match slot {
                    b'(' => 0,
                    b')' => 1,
                    b'*' => 2,
                    _ => 3,
                };
                match set {
                    b'B' => self
                        .state
                        .grid
                        .screen_mut()
                        .designate_charset(slot, Charset::Ascii),
                    b'0' => self
                        .state
                        .grid
                        .screen_mut()
                        .designate_charset(slot, Charset::SpecialCharacterAndLineDrawing),
                    _ => self.unhandled(),
                }
            }
            ([b'#'], b'8') => self.align_test(),
            ([], b'D') => self.line_feed(),
            ([], b'E') => {
                self.line_feed();
                self.state.grid.screen_mut().carriage_return();
            }
            ([], b'M') => {
                let report = self.state.grid.reverse_index();
                self.report(report);
            }
            ([], b'H') => {
                let screen = self.state.grid.screen_mut();
                let col = screen.cursor().pos.col;
                screen.tabs_mut().set(col);
            }
            ([], b'N') => self.state.shifts.single = Some(2),
            ([], b'O') => self.state.shifts.single = Some(3),
            ([], b'n') => self.state.shifts.active = 2,
            ([], b'o') => self.state.shifts.active = 3,
            ([], b'7') => self.state.save_cursor(),
            ([], b'8') => self.state.restore_cursor(),
            ([], b'c') => self.full_reset(),
            ([], b'Z') => self.reply_primary_attributes(),
            ([], b'=') => self.state.modes.set(Mode::AppKeypad, true),
            ([], b'>') => self.state.modes.set(Mode::AppKeypad, false),
            ([], b'\\') => {}
            _ => self.unhandled(),
        }
    }

    /// DECALN: fills the screen with `E`, default style; cursor and region stay.
    fn align_test(&mut self) {
        self.state.invalidate(Invalidation::EraseScreen);
        self.state.bump_generation();
        let screen = self.state.grid.screen_mut();
        let cell = Cell::EMPTY.with_content(CellContent::Scalar('E'));
        let cols = screen.cols();
        for index in 0..screen.rows() {
            let id = screen.row_of_index(index);
            if let Some(mut row) = screen.row_mut(id) {
                row.fill(0..cols, cell);
            }
        }
    }

    fn dcs_open(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if ignore {
            self.unhandled();
            return;
        }
        let query = match (intermediates, byte) {
            ([b'$'], b'q') => DcsQuery::Decrqss,
            ([b'+'], b'q') => DcsQuery::Xtgettcap,
            // Sixel: `q` alone. The DCS parameters (aspect, background, grid) are ignored.
            ([], b'q') => {
                self.state.graphics.decoder = Some(SixelDecoder::new());
                return;
            }
            _ => {
                self.unhandled();
                return;
            }
        };
        self.state.dcs_query = Some(query);
        self.state.dcs_payload.clear();
    }

    fn dcs_close(&mut self, aborted: bool) {
        let query = self.state.dcs_query.take();
        let decoder = self.state.graphics.decoder.take();
        if aborted {
            let stats = &mut self.state.stats;
            stats.aborted_dcs = stats.aborted_dcs.saturating_add(1);
            self.state.dcs_payload.clear();
            return;
        }
        if let Some(decoder) = decoder {
            if let Some(image) = decoder.finish() {
                self.place(image);
            }
            return;
        }
        let Some(query) = query else {
            return;
        };
        // The payload moves out so the answer can borrow it next to the reply buffers; moving it
        // back keeps its capacity for the next request.
        let mut payload = std::mem::take(&mut self.state.dcs_payload);
        if payload.len() >= QUERY_MAX_BYTES {
            self.unhandled();
        } else {
            self.answer_dcs_query(query, &payload);
        }
        payload.clear();
        self.state.dcs_payload = payload;
    }
}

impl Dispatch for Handler<'_> {
    fn print_str(&mut self, text: &str) {
        self.print_text(text);
    }

    fn execute(&mut self, byte: u8) {
        self.begin_control();
        self.control(byte);
    }

    fn esc(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        self.begin_control();
        self.esc_dispatch(intermediates, ignore, byte);
    }

    fn csi(&mut self, params: &Params, intermediates: &[u8], ignore: bool, byte: u8) {
        self.begin_control();
        self.csi_dispatch(params, intermediates, ignore, byte);
    }

    fn osc(
        &mut self,
        code: Option<u32>,
        params: &OscParams<'_>,
        term: StringTerm,
        truncated: bool,
    ) {
        self.begin_control();
        self.osc_dispatch(code, params, term, truncated);
    }

    fn osc_allows_large(&self, code: u32) -> bool {
        let routes = &self.state.config.osc_routes;
        routes.allows_large(code) && routes.get(code) != super::OscRoute::Drop
    }

    fn dcs_hook(&mut self, _params: &Params, intermediates: &[u8], ignore: bool, byte: u8) {
        self.begin_control();
        self.dcs_open(intermediates, ignore, byte);
    }

    fn dcs_put(&mut self, byte: u8) {
        if let Some(decoder) = &mut self.state.graphics.decoder {
            decoder.put(byte);
        } else if self.state.dcs_query.is_some() && self.state.dcs_payload.len() < QUERY_MAX_BYTES {
            self.state.dcs_payload.push(byte);
        }
    }

    fn dcs_unhook(&mut self, aborted: bool) {
        self.dcs_close(aborted);
    }

    fn apc_start(&mut self, _introducer: u8) {
        self.begin_control();
        self.unhandled();
    }
}
