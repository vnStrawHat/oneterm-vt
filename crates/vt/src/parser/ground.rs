//! The ground state: bulk scan for `ESC`, UTF-8 decoding with replacement, C1 routing.

use super::state::State;
use super::{Dispatch, Parser};

const ESC: u8 = 0x1B;
const REPLACEMENT: &str = "\u{FFFD}";

/// What ended a ground run, which decides the fate of a cut-off multi-byte character.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RunEnd {
    /// A byte that is not a continuation: the cut-off character is malformed.
    Byte,
    /// The end of the chunk: the character may continue in the next one.
    Chunk,
}

impl Parser {
    /// Consumes one ground run and returns the unread rest, which starts after an `ESC` if the
    /// run ended at one (the parser is then in `Escape`).
    pub(super) fn ground<'a, D: Dispatch + ?Sized>(
        &mut self,
        dispatch: &mut D,
        bytes: &'a [u8],
    ) -> &'a [u8] {
        let Some(index) = memchr::memchr(ESC, bytes) else {
            self.print_run(dispatch, bytes, RunEnd::Chunk);
            return &[];
        };
        let (run, tail) = bytes.split_at(index);
        self.print_run(dispatch, run, RunEnd::Byte);
        self.clear();
        self.state = State::Escape;
        tail.get(1..).unwrap_or_default()
    }

    /// Decodes a run without `ESC`. Valid text goes to `print_str`; each maximal invalid
    /// subpart becomes one replacement character, except a lone `0x80..=0x9F`, which is an
    /// 8-bit C1 control. Linear in the run length: every byte is examined once.
    fn print_run<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, run: &[u8], end: RunEnd) {
        let mut offset = 0;
        for chunk in run.utf8_chunks() {
            let invalid = chunk.invalid();
            offset += chunk.valid().len() + invalid.len();
            print_text(dispatch, chunk.valid());
            if invalid.is_empty() {
                continue;
            }
            let is_tail = offset == run.len();
            if is_tail && end == RunEnd::Chunk && is_cut_off(invalid) {
                self.carry_utf8(invalid);
            } else {
                print_invalid(dispatch, invalid);
            }
        }
    }

    fn carry_utf8(&mut self, bytes: &[u8]) {
        if let Some(slot) = self.utf8_carry.get_mut(..bytes.len()) {
            slot.copy_from_slice(bytes);
            self.utf8_carry_len = bytes.len() as u8;
        }
    }

    /// Completes a character cut by the previous chunk, using bytes from the start of `bytes`.
    /// Returns the bytes after the ones it used. The result equals decoding the joined stream.
    pub(super) fn finish_utf8_carry<'a, D: Dispatch + ?Sized>(
        &mut self,
        dispatch: &mut D,
        bytes: &'a [u8],
    ) -> &'a [u8] {
        // The carry is at most 3 bytes by construction; the clamp keeps the slice bound obvious.
        let carried = usize::from(self.utf8_carry_len).min(3);
        let take = bytes.len().min(4 - carried);
        let mut joined = self.utf8_carry;
        if let (Some(slot), Some(new)) =
            (joined.get_mut(carried..carried + take), bytes.get(..take))
        {
            slot.copy_from_slice(new);
        }
        let available = carried + take;
        let window = joined.get(..available).unwrap_or_default();
        let Some(first) = window.utf8_chunks().next() else {
            return bytes;
        };

        if let Some(c) = first.valid().chars().next() {
            self.utf8_carry_len = 0;
            match u8::try_from(c) {
                Ok(byte) if c.is_control() => dispatch.execute(byte),
                _ => dispatch.print(c),
            }
            return bytes
                .get(c.len_utf8().saturating_sub(carried)..)
                .unwrap_or_default();
        }
        let invalid = first.invalid();
        if invalid.len() == available && is_cut_off(invalid) {
            // Still incomplete: the new bytes were too few to finish it.
            self.utf8_carry = joined;
            self.utf8_carry_len = available as u8;
            return bytes.get(take..).unwrap_or_default();
        }
        self.utf8_carry_len = 0;
        dispatch.print_str(REPLACEMENT);
        // Often nothing: the byte that broke the character is read again from ground.
        bytes
            .get(invalid.len().saturating_sub(carried)..)
            .unwrap_or_default()
    }
}

/// Whether `bytes` (a maximal invalid subpart) is a valid prefix of a longer character.
fn is_cut_off(bytes: &[u8]) -> bool {
    str::from_utf8(bytes).is_err_and(|error| error.error_len().is_none())
}

fn print_invalid<D: Dispatch + ?Sized>(dispatch: &mut D, bytes: &[u8]) {
    match bytes {
        &[byte @ 0x80..=0x9F] => dispatch.execute(byte),
        _ => dispatch.print_str(REPLACEMENT),
    }
}

/// Prints valid text, routing C0, `DEL` and C1 (as `C2 80..=C2 9F`) to `execute`.
fn print_text<D: Dispatch + ?Sized>(dispatch: &mut D, text: &str) {
    let mut rest = text;
    while !rest.is_empty() {
        let bytes = rest.as_bytes();
        let split = bytes
            .iter()
            .enumerate()
            .find_map(|(index, &byte)| match byte {
                0x00..=0x1F | 0x7F => Some((index, 1, byte)),
                0xC2 => match bytes.get(index + 1) {
                    Some(&next @ 0x80..=0x9F) => Some((index, 2, next)),
                    _ => None,
                },
                _ => None,
            });
        let Some((index, width, control)) = split else {
            dispatch.print_str(rest);
            return;
        };
        if let Some(head) = rest.get(..index).filter(|head| !head.is_empty()) {
            dispatch.print_str(head);
        }
        dispatch.execute(control);
        rest = rest.get(index + width..).unwrap_or_default();
    }
}
