//! Memory and size caps, checked through the public API only.

use oneterm_vt::parser::{
    DCS_MAX_BYTES, Dispatch, MAX_INTERMEDIATES, MAX_PARAMS, OSC_INLINE, OSC_LARGE, OscParams,
    Params, Parser, StringTerm,
};

const MIB: usize = 1024 * 1024;

#[derive(Default)]
struct Limits {
    claims: Vec<u32>,
    puts: usize,
    ends: usize,
    aborted_ends: usize,
    osc_total: Option<usize>,
    osc_truncated: bool,
    csi_params_len: Option<usize>,
    csi_ignored: bool,
    csi_intermediates_len: Option<usize>,
}

impl Dispatch for Limits {
    fn print_str(&mut self, _text: &str) {}
    fn execute(&mut self, _byte: u8) {}

    fn csi(&mut self, params: &Params, intermediates: &[u8], ignore: bool, _byte: u8) {
        // The params table is the long one in the first sequence, the intermediates in the second.
        if params.len() > self.csi_params_len.unwrap_or(0) {
            self.csi_params_len = Some(params.len());
            self.csi_ignored = ignore;
        }
        if intermediates.len() > self.csi_intermediates_len.unwrap_or(0) {
            self.csi_intermediates_len = Some(intermediates.len());
            assert!(ignore);
        }
    }

    fn osc(
        &mut self,
        _code: Option<u32>,
        params: &OscParams<'_>,
        _term: StringTerm,
        truncated: bool,
    ) {
        self.osc_total = Some(params.iter().map(<[u8]>::len).sum());
        self.osc_truncated = truncated;
    }

    fn osc_allows_large(&self, code: u32) -> bool {
        self.claims.contains(&code)
    }

    fn dcs_put(&mut self, _byte: u8) {
        self.puts += 1;
    }

    fn dcs_unhook(&mut self, aborted: bool) {
        self.ends += 1;
        self.aborted_ends += usize::from(aborted);
    }

    fn apc_put(&mut self, _byte: u8) {
        self.puts += 1;
    }

    fn apc_end(&mut self, aborted: bool) {
        self.ends += 1;
        self.aborted_ends += usize::from(aborted);
    }
}

/// Feeds `header`, then `count` copies of `fill` in 64 KiB chunks, then `trailer`.
fn feed_stream(sink: &mut Limits, header: &[u8], fill: u8, count: usize, trailer: &[u8]) {
    let mut parser = Parser::new();
    parser.advance(sink, header);
    let chunk = vec![fill; 64 * 1024];
    let mut left = count;
    while left > 0 {
        let size = left.min(chunk.len());
        parser.advance(sink, &chunk[..size]);
        left -= size;
    }
    parser.advance(sink, trailer);
}

#[test]
fn dcs_payload_stops_at_cap_and_aborts_once() {
    let mut sink = Limits::default();
    feed_stream(&mut sink, b"\x1bPq", b'x', DCS_MAX_BYTES + 1000, b"\x1b\\");
    assert_eq!(sink.puts, DCS_MAX_BYTES);
    assert_eq!((sink.ends, sink.aborted_ends), (1, 1));
}

#[test]
fn dcs_payload_at_cap_is_not_aborted() {
    let mut sink = Limits::default();
    feed_stream(&mut sink, b"\x1bPq", b'x', DCS_MAX_BYTES, b"\x1b\\");
    assert_eq!(sink.puts, DCS_MAX_BYTES);
    assert_eq!((sink.ends, sink.aborted_ends), (1, 0));
}

#[test]
fn apc_payload_stops_at_cap_and_aborts_once() {
    let mut sink = Limits::default();
    feed_stream(&mut sink, b"\x1b_G", b'x', DCS_MAX_BYTES + 1000, b"\x1b\\");
    // The `G` is payload too.
    assert_eq!(sink.puts, DCS_MAX_BYTES);
    assert_eq!((sink.ends, sink.aborted_ends), (1, 1));
}

#[test]
fn osc_unclaimed_is_cut_at_inline() {
    let mut sink = Limits::default();
    feed_stream(&mut sink, b"\x1b]0;", b'x', MIB, b"\x07");
    assert_eq!(sink.osc_total, Some(OSC_INLINE));
    assert!(sink.osc_truncated);
}

#[test]
fn osc_claimed_is_cut_at_large() {
    let mut sink = Limits {
        claims: vec![52],
        ..Limits::default()
    };
    feed_stream(&mut sink, b"\x1b]52;c;", b'x', 9 * MIB, b"\x07");
    assert_eq!(sink.osc_total, Some(OSC_LARGE));
    assert!(sink.osc_truncated);
}

#[test]
fn osc_claim_is_per_code() {
    let mut sink = Limits {
        claims: vec![52],
        ..Limits::default()
    };
    feed_stream(&mut sink, b"\x1b]0;", b'x', 9 * MIB, b"\x07");
    assert_eq!(sink.osc_total, Some(OSC_INLINE));
}

#[test]
fn params_and_intermediates_stay_bounded() {
    let mut sink = Limits::default();
    let mut parser = Parser::new();
    let mut params_sequence = b"\x1b[".to_vec();
    params_sequence.extend(b";1".repeat(10_000));
    params_sequence.push(b'm');
    parser.advance(&mut sink, &params_sequence);
    let mut intermediates_sequence = b"\x1b[".to_vec();
    intermediates_sequence.extend([b' '; 100]);
    intermediates_sequence.push(b'q');
    parser.advance(&mut sink, &intermediates_sequence);

    assert_eq!(sink.csi_params_len, Some(MAX_PARAMS));
    assert!(sink.csi_ignored);
    assert_eq!(sink.csi_intermediates_len, Some(MAX_INTERMEDIATES));
}
