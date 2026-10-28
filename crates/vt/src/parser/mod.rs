//! A byte-stream parser for terminal output.
//!
//! [`Parser`] turns an untrusted byte stream into calls on a [`Dispatch`] sink: printable text,
//! control bytes, escape, CSI, OSC, DCS and APC sequences. It knows no grid and no terminal mode.
//!
//! The input may be hostile and arrive in chunks of any size. The parser never panics and never
//! returns an error: it truncates, drops or replaces what it cannot hold, and tells the sink
//! when it did (`ignore`, `truncated`, `aborted`). Feeding a stream in pieces yields the same
//! calls as feeding it whole.

mod ground;
mod osc;
mod params;
mod state;

#[cfg(test)]
mod parser_tests;

pub use osc::{MAX_OSC_PARAMS, OSC_INLINE, OSC_LARGE, OscParams, StringTerm};
pub use params::{MAX_INTERMEDIATES, MAX_PARAMS, ParamGroups, ParamSep, Params};

use osc::OscBuffer;
use params::Intermediates;
use state::State;

/// Most payload bytes a DCS or APC string passes to the sink; the next byte aborts the string.
pub const DCS_MAX_BYTES: usize = 16 * 1024 * 1024;

/// The receiver of everything a [`Parser`] recognises.
///
/// Calls happen in stream order, synchronously inside [`Parser::advance`]. Only `print_str` and
/// `execute` are required; every other method ignores its input by default.
pub trait Dispatch {
    /// A run of printable text. Never empty and never holds a control character
    /// (`U+0000..=U+001F`, `U+007F..=U+009F`); may hold `U+FFFD` for bad input. Where a run
    /// ends carries no meaning, because controls and chunk ends split runs.
    fn print_str(&mut self, text: &str);

    /// One character completed from bytes split across chunks. Same character rules as
    /// [`print_str`](Self::print_str); forwards to it by default.
    fn print(&mut self, c: char) {
        self.print_str(c.encode_utf8(&mut [0; 4]));
    }

    /// A control byte: C0, `DEL`, or C1 (`0x80..=0x9F`, raw or UTF-8 encoded) in ground; a C0
    /// inside an escape or CSI header; `CAN` and `SUB` anywhere, always. Never an introducer.
    fn execute(&mut self, byte: u8);

    /// The final byte of an escape sequence. `ignore` is set when a third intermediate arrived.
    fn esc(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}

    /// The final byte of a control sequence. `params` holds at least one value and
    /// `ignore == params.ignored()`: a limit was hit or the header was malformed, and the sink
    /// should drop the sequence. A private marker, if any, is the first intermediate.
    fn csi(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _byte: u8) {}

    /// An OSC ended by `BEL` or `ESC`. `params` holds at least one entry. `code` is the first
    /// entry read as a decimal `u32`, or `None` if it is empty, not all digits, or too large.
    /// `truncated` means bytes past the size cap were dropped. The slices are valid for this call
    /// only. An OSC cancelled by `CAN` or `SUB` is not reported.
    fn osc(
        &mut self,
        _code: Option<u32>,
        _params: &OscParams<'_>,
        _term: StringTerm,
        _truncated: bool,
    ) {
    }

    /// Asked once per OSC, when its first entry is complete and holds a code: may this OSC hold
    /// up to [`OSC_LARGE`] bytes instead of [`OSC_INLINE`]? The answer must depend on `code`
    /// only. It is a memory ceiling, not a policy.
    fn osc_allows_large(&self, _code: u32) -> bool {
        false
    }

    /// The final byte of a DCS header; the payload follows through `dcs_put`. Same parameter
    /// guarantees as [`csi`](Self::csi). An `ignore` hook still streams its payload.
    fn dcs_hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _byte: u8) {}

    /// One DCS payload byte: `0x00..=0x17`, `0x19`, `0x1C..=0x7E`. At most [`DCS_MAX_BYTES`]
    /// per hook.
    fn dcs_put(&mut self, _byte: u8) {}

    /// The end of a DCS string, exactly once per [`dcs_hook`](Self::dcs_hook) unless
    /// [`Parser::reset`] intervened. `aborted` is set when `CAN`/`SUB` cancelled it or the size
    /// cap was hit.
    fn dcs_unhook(&mut self, _aborted: bool) {}

    /// The start of an APC (`b'_'`), SOS (`b'X'`) or PM (`b'^'`) string.
    fn apc_start(&mut self, _introducer: u8) {}

    /// One APC, SOS or PM payload byte: any byte but `CAN`, `SUB` and `ESC`. At most
    /// [`DCS_MAX_BYTES`] per start.
    fn apc_put(&mut self, _byte: u8) {}

    /// The end of an APC, SOS or PM string, exactly once per [`apc_start`](Self::apc_start)
    /// unless [`Parser::reset`] intervened. `aborted` is set when `CAN`/`SUB` cancelled it or the
    /// size cap was hit.
    fn apc_end(&mut self, _aborted: bool) {}
}

/// The byte-stream parser. Cheap to create; the OSC buffer is its only heap allocation.
pub struct Parser {
    state: State,
    params: Params,
    intermediates: Intermediates,
    pending_value: u16,
    pending_sep: ParamSep,
    osc: OscBuffer,
    string_bytes: usize,
    string_aborted: bool,
    utf8_carry: [u8; 4],
    utf8_carry_len: u8,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    /// A parser in the ground state.
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            params: Params::new(),
            intermediates: Intermediates::new(),
            pending_value: 0,
            pending_sep: ParamSep::Semicolon,
            osc: OscBuffer::new(),
            string_bytes: 0,
            string_aborted: false,
            utf8_carry: [0; 4],
            utf8_carry_len: 0,
        }
    }

    /// Parses `bytes`, calling `dispatch` in stream order. A sequence or character may be split
    /// across calls at any byte.
    #[cfg_attr(feature = "hotpath-profiling", hotpath::measure(impl_type = "Parser"))]
    pub fn advance<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, bytes: &[u8]) {
        let mut rest = bytes;
        if self.utf8_carry_len > 0 {
            rest = self.finish_utf8_carry(dispatch, rest);
        }
        while !rest.is_empty() {
            if self.state == State::Ground {
                rest = self.ground(dispatch, rest);
                continue;
            }
            let mut iter = rest.iter();
            while self.state != State::Ground {
                let Some(&byte) = iter.next() else { break };
                // Parameter bytes are most of a CSI; they skip the full transition table.
                if matches!(self.state, State::CsiEntry | State::CsiParam)
                    && matches!(byte, 0x30..=0x3B)
                {
                    self.csi_parameter(byte);
                } else {
                    self.step(dispatch, byte);
                }
            }
            rest = iter.as_slice();
        }
    }

    /// Returns to the ground state and drops any partial sequence or character. The sink is not
    /// called: one hooked in a DCS or APC string gets no end call and must reset its own state.
    pub fn reset(&mut self) {
        self.state = State::Ground;
        self.clear();
        self.osc.start();
        self.string_bytes = 0;
        self.string_aborted = false;
        self.utf8_carry_len = 0;
    }

    /// Starts a new sequence: no parameters, intermediates or pending value.
    fn clear(&mut self) {
        self.params.clear();
        self.intermediates.clear();
        self.pending_value = 0;
        self.pending_sep = ParamSep::Semicolon;
    }

    /// Stores an intermediate or private marker; a third one flags the sequence.
    fn collect(&mut self, byte: u8) {
        if !self.intermediates.push(byte) {
            self.params.mark_ignored();
        }
    }

    fn digit(&mut self, byte: u8) {
        if self.params.is_full() {
            self.params.mark_ignored();
            return;
        }
        let digit = u16::from(byte - b'0');
        self.pending_value = self.pending_value.saturating_mul(10).saturating_add(digit);
    }

    fn separator(&mut self, sep: ParamSep) {
        self.push_pending_value();
        self.pending_sep = sep;
    }

    fn push_pending_value(&mut self) {
        self.params.push(self.pending_value, self.pending_sep);
        self.pending_value = 0;
    }

    fn dispatch_csi<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, byte: u8) {
        self.push_pending_value();
        dispatch.csi(
            &self.params,
            self.intermediates.as_slice(),
            self.params.ignored(),
            byte,
        );
        self.state = State::Ground;
    }

    fn hook_dcs<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, byte: u8) {
        self.push_pending_value();
        dispatch.dcs_hook(
            &self.params,
            self.intermediates.as_slice(),
            self.params.ignored(),
            byte,
        );
        self.string_bytes = 0;
        self.string_aborted = false;
        self.state = State::DcsPassthrough;
    }

    fn finish_osc<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, term: StringTerm) {
        if self.osc.close_param() {
            self.osc
                .resolve_code(|code| dispatch.osc_allows_large(code));
        }
        dispatch.osc(
            self.osc.code(),
            &self.osc.params(),
            term,
            self.osc.truncated(),
        );
        self.osc.start();
    }

    fn osc_semicolon<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D) {
        if self.osc.semicolon() {
            self.osc
                .resolve_code(|code| dispatch.osc_allows_large(code));
        }
    }
}
