//! CSI dispatch: cursor motion, erasing, scrolling, tab stops, regions and the routing of
//! every other final byte to the module that owns it.

use crate::events::VtEvent;
use crate::grid::{DisplayClear, LineClear, TabStops};
use crate::parser::{ParamGroups, Params};
use crate::selection::Invalidation;

use super::dispatch::Handler;

/// The parameters of one sequence, read group by group.
///
/// A group is the values between two `;`; only SGR looks inside one (sub-parameters).
pub(super) struct Args<'a> {
    groups: ParamGroups<'a>,
}

impl<'a> Args<'a> {
    pub(super) fn new(params: &'a Params) -> Args<'a> {
        Args {
            groups: params.groups(),
        }
    }

    /// The first value of the next group; `None` when the sequence has no more groups.
    pub(super) fn next_raw(&mut self) -> Option<u16> {
        self.groups.next().and_then(|group| group.first().copied())
    }

    /// The first value of the next group, or `default` when it is absent or `0`.
    pub(super) fn next_or(&mut self, default: u16) -> u16 {
        match self.next_raw() {
            Some(value) if value != 0 => value,
            _ => default,
        }
    }

    /// The groups not read yet.
    pub(super) fn groups(&self) -> ParamGroups<'a> {
        self.groups.clone()
    }
}

impl Handler<'_> {
    pub(super) fn csi_dispatch(
        &mut self,
        params: &Params,
        intermediates: &[u8],
        ignore: bool,
        byte: u8,
    ) {
        if ignore {
            self.unhandled();
            return;
        }
        let mut args = Args::new(params);
        let args = &mut args;
        match (intermediates, byte) {
            ([], b'@') => {
                let n = args.next_or(1);
                self.state.grid.screen_mut().insert_blanks(n);
            }
            ([], b'A') => self.cursor_up(args.next_or(1), false),
            ([], b'B' | b'e') => self.cursor_down(args.next_or(1), false),
            ([], b'C' | b'a') => {
                let n = args.next_or(1);
                let screen = self.state.grid.screen_mut();
                let (index, col) = (screen.cursor_row_index(), screen.cursor().pos.col);
                screen.goto(index, col.saturating_add(n));
            }
            ([], b'D') => {
                let n = args.next_or(1);
                let screen = self.state.grid.screen_mut();
                let (index, col) = (screen.cursor_row_index(), screen.cursor().pos.col);
                screen.goto(index, col.saturating_sub(n));
            }
            ([], b'E') => self.cursor_down(args.next_or(1), true),
            ([], b'F') => self.cursor_up(args.next_or(1), true),
            ([], b'G' | b'`') => {
                let col = args.next_or(1) - 1;
                let screen = self.state.grid.screen_mut();
                let index = screen.cursor_row_index();
                screen.goto(index, col);
            }
            ([], b'd') => {
                let row = args.next_or(1) - 1;
                let origin = self.origin();
                let screen = self.state.grid.screen_mut();
                let col = screen.cursor().pos.col;
                screen.goto_origin(row, col, origin);
            }
            ([], b'H' | b'f') => {
                let row = args.next_or(1) - 1;
                let col = args.next_or(1) - 1;
                let origin = self.origin();
                self.state.grid.screen_mut().goto_origin(row, col, origin);
            }
            ([], b'I') => {
                let n = args.next_or(1);
                self.state.grid.screen_mut().tab_forward(n);
            }
            ([], b'Z') => {
                let n = args.next_or(1);
                self.state.grid.screen_mut().tab_backward(n);
            }
            ([], b'J') => self.erase_display(args.next_raw().unwrap_or(0)),
            ([], b'K') => self.erase_line(args.next_raw().unwrap_or(0)),
            ([], b'L') => {
                let n = args.next_or(1);
                let report = self.state.grid.insert_lines(n);
                self.report(report);
            }
            ([], b'M') => {
                let n = args.next_or(1);
                let report = self.state.grid.delete_lines(n);
                let rows = self.report(report);
                self.add_lines_produced(rows);
            }
            ([], b'P') => {
                let n = args.next_or(1);
                self.state.grid.screen_mut().delete_chars(n);
            }
            ([], b'X') => {
                let n = args.next_or(1);
                self.state.grid.screen_mut().erase_chars(n);
            }
            ([], b'S') => {
                let n = args.next_or(1);
                let region = self.state.grid.screen_mut().region();
                let report = self.state.grid.scroll_up(region, n);
                let rows = self.report(Some(report));
                self.add_lines_produced(rows);
            }
            ([], b'T') => {
                let n = args.next_or(1);
                let region = self.state.grid.screen_mut().region();
                let report = self.state.grid.scroll_down(region, n);
                self.report(Some(report));
            }
            ([], b'b') => {
                let n = args.next_or(1);
                self.repeat_preceding(n);
            }
            ([], b'c') => self.device_attributes(args, DeviceAttributes::Primary),
            ([b'>'], b'c') => self.device_attributes(args, DeviceAttributes::Secondary),
            ([b'='], b'c') => self.device_attributes(args, DeviceAttributes::Tertiary),
            ([], b'g') => self.clear_tab_stops(args.next_raw().unwrap_or(0)),
            ([b'?'], b'W') => self.reset_tab_stops(args.next_raw().unwrap_or(0)),
            ([], b'h') => self.set_modes(args, false, true),
            ([], b'l') => self.set_modes(args, false, false),
            ([b'?'], b'h') => self.set_modes(args, true, true),
            ([b'?'], b'l') => self.set_modes(args, true, false),
            ([], b'm') => self.sgr(args),
            ([b'>'], b'm') => self.set_modify_other_keys(args),
            ([b'?'], b'm') => self.query_modify_other_keys(args),
            ([], b'n') => self.status_report(args, false),
            ([b'?'], b'n') => self.status_report(args, true),
            ([b'!'], b'p') => self.soft_reset(),
            ([b'$'], b'p') => self.report_mode(args, false),
            ([b'?', b'$'], b'p') => self.report_mode(args, true),
            ([b'>'], b'q') => self.report_version(args),
            ([b' '], b'q') => self.set_cursor_style(args),
            ([], b'r') => self.set_scroll_region(args),
            ([], b's') => self.state.save_cursor(),
            ([], b'u') => self.state.restore_cursor(),
            ([], b't') => self.window_operation(args),
            ([b'?'], b'u') => self.query_keyboard_flags(),
            ([b'='], b'u') => {
                let flags = args.next_or(0);
                let how = args.next_or(1);
                self.state.keyboard.active.set(flags, how);
            }
            ([b'>'], b'u') => {
                let flags = args.next_or(0);
                self.state.keyboard.active.push(flags);
            }
            ([b'<'], b'u') => {
                let n = args.next_or(1);
                self.state.keyboard.active.pop(n);
            }
            ([b'*'], b'y') => self.checksum_rectangle(args),
            _ => self.unhandled(),
        }
    }

    fn add_lines_produced(&mut self, rows: u16) {
        self.state.lines_produced = self.state.lines_produced.saturating_add(u64::from(rows));
    }

    /// CUU and CPL: up `n` rows, stopping at the region top when inside the region and at the
    /// screen top otherwise.
    fn cursor_up(&mut self, n: u16, to_column_zero: bool) {
        let screen = self.state.grid.screen_mut();
        let (index, col) = (screen.cursor_row_index(), screen.cursor().pos.col);
        let top = screen.region().top;
        let floor = if index >= top { top } else { 0 };
        let col = if to_column_zero { 0 } else { col };
        screen.goto(index.saturating_sub(n).max(floor), col);
    }

    /// CUD, VPR and CNL: down `n` rows, stopping at the region bottom when inside the region
    /// and at the screen bottom otherwise.
    fn cursor_down(&mut self, n: u16, to_column_zero: bool) {
        let screen = self.state.grid.screen_mut();
        let (index, col) = (screen.cursor_row_index(), screen.cursor().pos.col);
        let bottom = screen.region().bottom;
        let ceiling = if index < bottom {
            bottom - 1
        } else {
            screen.rows() - 1
        };
        let col = if to_column_zero { 0 } else { col };
        screen.goto(index.saturating_add(n).min(ceiling), col);
    }

    fn erase_display(&mut self, selector: u16) {
        let (mode, invalidation) = match selector {
            0 => (DisplayClear::Below, Invalidation::EraseBelow),
            1 => (DisplayClear::Above, Invalidation::EraseAbove),
            2 => (DisplayClear::All, Invalidation::EraseScreen),
            3 => (DisplayClear::Saved, Invalidation::EraseHistory),
            _ => {
                self.unhandled();
                return;
            }
        };
        self.state.invalidate(invalidation);
        if matches!(mode, DisplayClear::All | DisplayClear::Saved) {
            self.out.push(VtEvent::ScreenCleared);
        }
        let report = self.state.grid.erase_display(mode, &self.state.interner);
        self.report(report);
    }

    fn erase_line(&mut self, selector: u16) {
        let mode = match selector {
            0 => LineClear::Right,
            1 => LineClear::Left,
            2 => LineClear::All,
            _ => {
                self.unhandled();
                return;
            }
        };
        self.state.invalidate(Invalidation::EraseLine);
        self.state.grid.screen_mut().erase_line(mode);
    }

    /// TBC.
    fn clear_tab_stops(&mut self, selector: u16) {
        let screen = self.state.grid.screen_mut();
        match selector {
            0 => {
                let col = screen.cursor().pos.col;
                screen.tabs_mut().clear(col);
            }
            3 => screen.tabs_mut().clear_all(),
            _ => self.unhandled(),
        }
    }

    /// DECST8C: a tab stop every 8 columns again.
    fn reset_tab_stops(&mut self, selector: u16) {
        if selector != 5 {
            self.unhandled();
            return;
        }
        let screen = self.state.grid.screen_mut();
        let cols = screen.cols();
        *screen.tabs_mut() = TabStops::new(cols);
    }

    /// DECSTBM. An empty or reversed region is ignored without a count.
    fn set_scroll_region(&mut self, args: &mut Args<'_>) {
        let top = args.next_or(1);
        let screen = self.state.grid.screen_mut();
        let bottom = match args.next_raw() {
            None | Some(0) => screen.rows(),
            Some(bottom) => bottom,
        };
        if top >= bottom {
            return;
        }
        let origin = self.origin();
        // `set_region` clamps the bottom to the screen and refuses an empty result.
        self.state
            .grid
            .screen_mut()
            .set_region(top - 1, bottom, origin);
    }
}

/// Which `CSI c` was asked.
#[derive(Clone, Copy)]
pub(super) enum DeviceAttributes {
    Primary,
    Secondary,
    Tertiary,
}
