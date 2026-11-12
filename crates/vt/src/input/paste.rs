//! Pasted text as the bytes a program expects.

use crate::snapshot::ModeSnapshot;

const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// The bytes for pasted `text`, or an empty `Vec` when nothing is left to send.
///
/// Control characters other than tab, line feed and carriage return are dropped, so pasted
/// text can neither end a bracketed paste early (`ESC [ 201 ~`) nor carry an escape sequence
/// or a `Ctrl-C` of its own. With bracketed paste (`?2004`) the rest goes between
/// `ESC [ 200 ~` and `ESC [ 201 ~` as it is; without it every line end (`\r\n` or `\n`)
/// becomes `\r`, the byte Enter sends.
///
/// ```
/// use oneterm_vt::ModeSnapshot;
/// use oneterm_vt::input::encode_paste;
///
/// let mut modes = ModeSnapshot::default();
/// assert_eq!(encode_paste("ls\nexit\n", &modes), b"ls\rexit\r");
/// modes.bracketed_paste = true; // `CSI ? 2004 h`
/// // The pasted text cannot close the bracket itself: its ESC is gone.
/// assert_eq!(encode_paste("a\x1b[201~b", &modes), b"\x1b[200~a[201~b\x1b[201~");
/// ```
pub fn encode_paste(text: &str, modes: &ModeSnapshot) -> Vec<u8> {
    let bracketed = modes.bracketed_paste;
    let mut body = Vec::with_capacity(text.len() + PASTE_START.len() + PASTE_END.len());
    if bracketed {
        body.extend_from_slice(PASTE_START);
    }
    let start = body.len();
    let mut utf8 = [0; 4];
    let mut previous = '\0';
    for c in text.chars() {
        if c.is_control() && !matches!(c, '\t' | '\n' | '\r') {
            continue;
        }
        match c {
            // The `\r` of a `\r\n` pair was sent already.
            '\n' if !bracketed && previous == '\r' => {}
            '\n' if !bracketed => body.push(b'\r'),
            _ => body.extend_from_slice(c.encode_utf8(&mut utf8).as_bytes()),
        }
        previous = c;
    }
    if body.len() == start {
        return Vec::new();
    }
    if bracketed {
        body.extend_from_slice(PASTE_END);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(bracketed_paste: bool) -> ModeSnapshot {
        ModeSnapshot {
            bracketed_paste,
            ..ModeSnapshot::default()
        }
    }

    #[test]
    fn bracketed_paste_is_wrapped_and_keeps_line_ends() {
        let bytes = encode_paste("one\r\ntwo\n\tthree", &modes(true));
        assert_eq!(bytes, b"\x1b[200~one\r\ntwo\n\tthree\x1b[201~");
    }

    #[test]
    fn pasted_text_cannot_close_the_bracket_or_send_controls() {
        let hostile = "a\x1b[201~rm -rf\x03\x7f\u{9b}201~b";
        assert_eq!(
            encode_paste(hostile, &modes(true)),
            b"\x1b[200~a[201~rm -rf201~b\x1b[201~"
        );
        assert_eq!(encode_paste(hostile, &modes(false)), b"a[201~rm -rf201~b");
    }

    #[test]
    fn plain_paste_turns_line_ends_into_carriage_returns() {
        assert_eq!(encode_paste("a\nb\r\nc\rd", &modes(false)), b"a\rb\rc\rd");
        assert_eq!(encode_paste("\n\n", &modes(false)), b"\r\r");
    }

    #[test]
    fn nothing_to_paste_sends_nothing() {
        for bracketed in [false, true] {
            assert!(encode_paste("", &modes(bracketed)).is_empty());
            assert!(encode_paste("\x1b\x00\x07", &modes(bracketed)).is_empty());
        }
        assert_eq!(
            encode_paste("é", &modes(true)),
            "\x1b[200~é\x1b[201~".as_bytes()
        );
    }
}
