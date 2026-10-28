//! The non-ground states: one function per state family, following the VT500 transition tables.

use super::{DCS_MAX_BYTES, Dispatch, ParamSep, Parser, StringTerm};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum State {
    Ground,
    Escape,
    EscapeIntermediate,
    CsiEntry,
    CsiParam,
    CsiIntermediate,
    CsiIgnore,
    DcsEntry,
    DcsParam,
    DcsIntermediate,
    DcsPassthrough,
    DcsIgnore,
    OscString,
    ApcString,
}

/// Which kind of sequence a header belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Csi,
    Dcs,
}

/// How far a header has got.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Entry,
    Param,
    Intermediate,
    Ignore,
}

impl State {
    fn header(family: Family, phase: Phase) -> Self {
        match (family, phase) {
            (Family::Csi, Phase::Entry) => Self::CsiEntry,
            (Family::Csi, Phase::Param) => Self::CsiParam,
            (Family::Csi, Phase::Intermediate) => Self::CsiIntermediate,
            (Family::Csi, Phase::Ignore) => Self::CsiIgnore,
            (Family::Dcs, Phase::Entry) => Self::DcsEntry,
            (Family::Dcs, Phase::Param) => Self::DcsParam,
            (Family::Dcs, Phase::Intermediate) => Self::DcsIntermediate,
            (Family::Dcs, Phase::Ignore) => Self::DcsIgnore,
        }
    }
}

/// What to do with the next DCS or APC payload byte.
enum StringByte {
    Pass,
    CapReached,
    Drop,
}

impl Parser {
    /// Feeds one byte to the current non-ground state.
    pub(super) fn step<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, byte: u8) {
        let state = self.state;
        match state {
            // Ground runs are handled in bulk; reaching here is a bug, and a no-op in release.
            State::Ground => debug_assert!(false, "ground byte reached the state table"),
            State::OscString => self.osc_string(dispatch, byte),
            State::DcsPassthrough => self.dcs_passthrough(dispatch, byte),
            State::ApcString => self.apc_string(dispatch, byte),
            _ if self.anywhere(dispatch, byte) => {}
            State::Escape | State::EscapeIntermediate => self.escape(dispatch, byte),
            State::CsiEntry => self.header(dispatch, byte, Family::Csi, Phase::Entry),
            State::CsiParam => self.header(dispatch, byte, Family::Csi, Phase::Param),
            State::CsiIntermediate => self.header(dispatch, byte, Family::Csi, Phase::Intermediate),
            State::CsiIgnore => self.header(dispatch, byte, Family::Csi, Phase::Ignore),
            State::DcsEntry => self.header(dispatch, byte, Family::Dcs, Phase::Entry),
            State::DcsParam => self.header(dispatch, byte, Family::Dcs, Phase::Param),
            State::DcsIntermediate => self.header(dispatch, byte, Family::Dcs, Phase::Intermediate),
            State::DcsIgnore => self.header(dispatch, byte, Family::Dcs, Phase::Ignore),
        }
    }

    /// Rules shared by the escape and header states. Returns whether the byte was consumed.
    fn anywhere<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, byte: u8) -> bool {
        match byte {
            0x18 | 0x1A => {
                dispatch.execute(byte);
                self.state = State::Ground;
            }
            0x1B => {
                self.clear();
                self.state = State::Escape;
            }
            // An 8-bit C1 byte is never an introducer here.
            0x80..=0xFF => {}
            _ => return false,
        }
        true
    }

    fn escape<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, byte: u8) {
        let in_intermediates = self.state == State::EscapeIntermediate;
        match byte {
            0x00..=0x17 | 0x19 | 0x1C..=0x1F => dispatch.execute(byte),
            0x20..=0x2F => {
                self.collect(byte);
                self.state = State::EscapeIntermediate;
            }
            b'P' if !in_intermediates => {
                self.clear();
                self.state = State::DcsEntry;
            }
            b'[' if !in_intermediates => {
                self.clear();
                self.state = State::CsiEntry;
            }
            b']' if !in_intermediates => {
                self.osc.start();
                self.state = State::OscString;
            }
            b'X' | b'^' | b'_' if !in_intermediates => {
                dispatch.apc_start(byte);
                self.string_bytes = 0;
                self.string_aborted = false;
                self.state = State::ApcString;
            }
            0x30..=0x7E => {
                dispatch.esc(self.intermediates.as_slice(), self.params.ignored(), byte);
                self.state = State::Ground;
            }
            _ => {}
        }
    }

    /// One byte of a CSI or DCS header, at the given phase.
    fn header<D: Dispatch + ?Sized>(
        &mut self,
        dispatch: &mut D,
        byte: u8,
        family: Family,
        phase: Phase,
    ) {
        match byte {
            // Inside a DCS header C0 bytes are dropped, inside a CSI header they act.
            0x00..=0x17 | 0x19 | 0x1C..=0x1F => {
                if family == Family::Csi {
                    dispatch.execute(byte);
                }
            }
            0x20..=0x2F if phase != Phase::Ignore => {
                self.collect(byte);
                self.state = State::header(family, Phase::Intermediate);
            }
            0x30..=0x3B => match phase {
                Phase::Entry | Phase::Param => {
                    self.parameter_byte(byte);
                    self.state = State::header(family, Phase::Param);
                }
                Phase::Intermediate => self.start_ignoring(family),
                Phase::Ignore => {}
            },
            0x3C..=0x3F => match phase {
                // A private marker is only valid straight after the introducer.
                Phase::Entry => {
                    self.collect(byte);
                    self.state = State::header(family, Phase::Param);
                }
                Phase::Param | Phase::Intermediate => self.start_ignoring(family),
                Phase::Ignore => {}
            },
            0x40..=0x7E => {
                if phase == Phase::Ignore {
                    self.params.mark_ignored();
                }
                match family {
                    Family::Csi => self.dispatch_csi(dispatch, byte),
                    Family::Dcs => self.hook_dcs(dispatch, byte),
                }
            }
            _ => {}
        }
    }

    /// A digit or separator (`0x30..=0x3B`) in a CSI header that has not seen an intermediate:
    /// the same transition `header` makes, without its dispatch on family and phase.
    pub(super) fn csi_parameter(&mut self, byte: u8) {
        self.parameter_byte(byte);
        self.state = State::CsiParam;
    }

    fn parameter_byte(&mut self, byte: u8) {
        match byte {
            b'0'..=b'9' => self.digit(byte),
            b':' => self.separator(ParamSep::Colon),
            _ => self.separator(ParamSep::Semicolon),
        }
    }

    /// A malformed header still dispatches once at its final byte, flagged as ignored.
    fn start_ignoring(&mut self, family: Family) {
        self.params.mark_ignored();
        self.state = State::header(family, Phase::Ignore);
    }

    fn osc_string<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, byte: u8) {
        match byte {
            0x07 => {
                self.finish_osc(dispatch, StringTerm::Bel);
                self.state = State::Ground;
            }
            0x18 | 0x1A => {
                // Cancelled: the payload is discarded, only the control byte is reported.
                self.osc.start();
                dispatch.execute(byte);
                self.state = State::Ground;
            }
            0x1B => {
                self.finish_osc(dispatch, StringTerm::St);
                self.clear();
                self.state = State::Escape;
            }
            b';' => self.osc_semicolon(dispatch),
            // `0x9C` is payload, not a terminator: UTF-8 text carries it inside characters.
            0x20..=0xFF => self.osc.push(byte),
            _ => {}
        }
    }

    fn dcs_passthrough<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, byte: u8) {
        match byte {
            0x18 | 0x1A => {
                if !self.string_aborted {
                    dispatch.dcs_unhook(true);
                }
                dispatch.execute(byte);
                self.state = State::Ground;
            }
            0x1B => {
                if !self.string_aborted {
                    dispatch.dcs_unhook(false);
                }
                self.clear();
                self.state = State::Escape;
            }
            // DCS payload is 7-bit: `DEL` and high bytes are dropped.
            0x00..=0x17 | 0x19 | 0x1C..=0x7E => match self.take_string_byte() {
                StringByte::Pass => dispatch.dcs_put(byte),
                StringByte::CapReached => dispatch.dcs_unhook(true),
                StringByte::Drop => {}
            },
            _ => {}
        }
    }

    fn apc_string<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, byte: u8) {
        match byte {
            0x18 | 0x1A => {
                if !self.string_aborted {
                    dispatch.apc_end(true);
                }
                dispatch.execute(byte);
                self.state = State::Ground;
            }
            0x1B => {
                if !self.string_aborted {
                    dispatch.apc_end(false);
                }
                self.clear();
                self.state = State::Escape;
            }
            _ => match self.take_string_byte() {
                StringByte::Pass => dispatch.apc_put(byte),
                StringByte::CapReached => dispatch.apc_end(true),
                StringByte::Drop => {}
            },
        }
    }

    /// Counts a DCS or APC payload byte against `DCS_MAX_BYTES`. The byte that reaches the cap
    /// aborts the string once; everything after it is dropped.
    fn take_string_byte(&mut self) -> StringByte {
        if self.string_aborted {
            return StringByte::Drop;
        }
        if self.string_bytes >= DCS_MAX_BYTES {
            self.string_aborted = true;
            return StringByte::CapReached;
        }
        self.string_bytes += 1;
        StringByte::Pass
    }
}
