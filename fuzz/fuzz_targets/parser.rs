//! Fuzzes `parser::Parser` alone, through a sink that touches every argument of every callback.
//!
//! The input is cut in two at a data-chosen byte, so parser state survives a chunk boundary.
//! Run: `cargo +nightly-2026-05-25 fuzz run --target-dir target/fuzz-linux parser`

#![no_main]

use std::hint::black_box;

use libfuzzer_sys::fuzz_target;
use oneterm_vt::StringTerm;
use oneterm_vt::parser::{Dispatch, Intermediates, OscParams, Params, Parser};

/// Reads every value the parser hands over, so a bad slice or index panics here.
struct TouchingSink;

impl TouchingSink {
    fn touch_params(params: &Params) {
        black_box(params.ignored());
        black_box(params.values());
        for index in 0..params.len() {
            black_box(params.get(index));
            black_box(params.sep(index));
        }
        for group in params.groups() {
            black_box(group);
        }
    }
}

impl Dispatch for TouchingSink {
    fn print_str(&mut self, text: &str) {
        black_box(text);
    }

    fn print(&mut self, c: char) {
        black_box(c);
    }

    fn execute(&mut self, byte: u8) {
        black_box(byte);
    }

    fn esc(&mut self, intermediates: &Intermediates, ignore: bool, byte: u8) {
        black_box((intermediates.as_slice(), ignore, byte));
    }

    fn csi(&mut self, params: &Params, intermediates: &Intermediates, ignore: bool, byte: u8) {
        Self::touch_params(params);
        black_box((intermediates.as_slice(), ignore, byte));
    }

    fn osc(
        &mut self,
        code: Option<u32>,
        params: &OscParams<'_>,
        term: StringTerm,
        truncated: bool,
    ) {
        for param in params.iter() {
            black_box(param);
        }
        black_box((code, params.len(), term, truncated));
    }

    fn osc_allows_large(&self, code: u32) -> bool {
        // Alternate by code so both buffer sizes are reached; the answer depends on `code` only.
        black_box(code).is_multiple_of(2)
    }

    fn dcs_hook(&mut self, params: &Params, intermediates: &Intermediates, ignore: bool, byte: u8) {
        Self::touch_params(params);
        black_box((intermediates.as_slice(), ignore, byte));
    }

    fn dcs_put(&mut self, byte: u8) {
        black_box(byte);
    }

    fn dcs_unhook(&mut self, aborted: bool) {
        black_box(aborted);
    }

    fn apc_start(&mut self, introducer: u8) {
        black_box(introducer);
    }

    fn apc_put(&mut self, byte: u8) {
        black_box(byte);
    }

    fn apc_end(&mut self, aborted: bool) {
        black_box(aborted);
    }
}

fuzz_target!(|data: &[u8]| {
    let Some(&first) = data.first() else {
        return;
    };
    let (head, tail) = data.split_at(usize::from(first) % data.len());
    let mut parser = Parser::new();
    let mut sink = TouchingSink;
    parser.advance(&mut sink, head);
    parser.advance(&mut sink, tail);
});
