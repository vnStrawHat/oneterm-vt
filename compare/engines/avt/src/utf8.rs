//! Chunked UTF-8 decoding for an engine that takes `&str` only.
//!
//! A chunk that is valid UTF-8 goes to the sink as a borrowed slice of itself: no copy. Only a
//! character split across chunks is joined, in a 4-byte stack buffer. Every invalid sequence
//! becomes one U+FFFD, the same text `String::from_utf8_lossy` would give for the whole stream.

/// Holds the incomplete tail of the previous chunk (at most 3 bytes).
#[derive(Default)]
pub struct Utf8Feeder {
    carry: [u8; 3],
    carried: usize,
}

/// Bytes of the sequence a valid lead byte starts.
fn sequence_len(lead: u8) -> usize {
    match lead {
        0xF0..=0xF4 => 4,
        0xE0..=0xEF => 3,
        _ => 2,
    }
}

impl Utf8Feeder {
    /// Sends `chunk` to `sink` as text, in order, holding back an incomplete tail.
    pub fn push(&mut self, chunk: &[u8], sink: &mut impl FnMut(&str)) {
        let mut rest = chunk;
        if self.carried > 0 {
            // Complete the held character with the head of this chunk. The joined bytes can
            // end in a new incomplete tail (the head may itself start a character).
            let missing = sequence_len(self.carry[0]) - self.carried;
            let take = missing.min(rest.len());
            let mut joined = [0u8; 4];
            joined[..self.carried].copy_from_slice(&self.carry[..self.carried]);
            joined[self.carried..self.carried + take].copy_from_slice(&rest[..take]);
            let length = self.carried + take;
            self.carried = 0;
            rest = &rest[take..];
            self.decode(&joined[..length], sink);
        }
        self.decode(rest, sink);
    }

    fn decode(&mut self, bytes: &[u8], sink: &mut impl FnMut(&str)) {
        let mut parts = bytes.utf8_chunks().peekable();
        while let Some(part) = parts.next() {
            if !part.valid().is_empty() {
                sink(part.valid());
            }
            let invalid = part.invalid();
            if invalid.is_empty() {
                continue;
            }
            let incomplete = std::str::from_utf8(invalid).is_err_and(|e| e.error_len().is_none());
            if incomplete && parts.peek().is_none() && invalid.len() <= self.carry.len() {
                self.carry[..invalid.len()].copy_from_slice(invalid);
                self.carried = invalid.len();
            } else {
                sink("\u{FFFD}");
            }
        }
    }

    /// Bytes held back, waiting for the rest of a character.
    #[cfg(test)]
    fn pending(&self) -> usize {
        self.carried
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(chunks: &[&[u8]]) -> String {
        let mut feeder = Utf8Feeder::default();
        let mut text = String::new();
        for chunk in chunks {
            feeder.push(chunk, &mut |s| text.push_str(s));
        }
        text
    }

    #[test]
    fn a_four_byte_char_split_over_two_and_three_chunks_decodes_once() {
        let rocket = "\u{1F680}".as_bytes();
        assert_eq!(
            decode(&[b"a", &rocket[..2], &rocket[2..], b"b"]),
            "a\u{1F680}b"
        );
        assert_eq!(
            decode(&[&rocket[..1], &rocket[1..3], &rocket[3..], b"z"]),
            "\u{1F680}z"
        );
        assert_eq!(decode(&[&rocket[..3], &rocket[3..]]), "\u{1F680}");
    }

    #[test]
    fn an_invalid_byte_becomes_one_replacement_char() {
        assert_eq!(decode(&[b"a\xFFb"]), "a\u{FFFD}b");
        // A lead byte followed by ASCII, across a chunk boundary and inside one.
        assert_eq!(decode(&[b"a\xE2", b"b"]), "a\u{FFFD}b");
        assert_eq!(decode(&[b"a\xE2b"]), "a\u{FFFD}b");
        // A held lead byte joined with another lead byte: the second starts a character.
        let euro = "\u{20AC}".as_bytes();
        assert_eq!(
            decode(&[b"\xE2", &[b'x', euro[0]], &euro[1..]]),
            "\u{FFFD}x\u{20AC}"
        );
    }

    #[test]
    fn the_stream_matches_lossy_decoding_wherever_it_is_cut() {
        let stream: &[u8] = b"ab\xC3\xA9\xE2\x82\xAC\xFF\xF0\x9F\x9A\x80\xE2\x82x\xC0\xAF end";
        let expected = String::from_utf8_lossy(stream).into_owned();
        for first in 0..=stream.len() {
            for second in first..=stream.len() {
                let parts = [&stream[..first], &stream[first..second], &stream[second..]];
                assert_eq!(decode(&parts), expected, "cut at {first} and {second}");
            }
        }
    }

    #[test]
    fn a_valid_chunk_reaches_the_sink_as_a_borrowed_slice() {
        let chunk = "plain text, no copy".as_bytes();
        let mut feeder = Utf8Feeder::default();
        let mut seen = Vec::new();
        feeder.push(chunk, &mut |s| seen.push((s.as_ptr(), s.len())));
        assert_eq!(seen, [(chunk.as_ptr(), chunk.len())]);
        assert_eq!(feeder.pending(), 0);
    }

    #[test]
    fn a_trailing_partial_char_is_held_not_emitted() {
        let mut feeder = Utf8Feeder::default();
        let mut text = String::new();
        feeder.push(b"ok\xE2\x82", &mut |s| text.push_str(s));
        assert_eq!(text, "ok");
        assert_eq!(feeder.pending(), 2);
    }
}
