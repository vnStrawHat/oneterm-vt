//! The regex matcher: the caller's `Regex` over each logical line of the copied text.

use super::{GridText, MAX_MATCHES, SearchMatch};

/// Finds every match of `regex`, one logical line at a time, at most `MAX_MATCHES`.
///
/// A `'\n'` ends every logical line, so a match never crosses one and `^`/`$` anchor to a line.
pub(super) fn search(text: &GridText, regex: &::regex::Regex) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    let mut line_start = 0;
    for newline in memchr::memchr_iter(b'\n', text.text.as_bytes()) {
        let Some(line) = text.text.get(line_start..newline) else {
            break;
        };
        for found in regex.find_iter(line) {
            let Some(span) = text.span(line_start + found.start(), line_start + found.end()) else {
                return matches;
            };
            matches.push(span);
            if matches.len() == MAX_MATCHES {
                return matches;
            }
        }
        line_start = newline + 1;
    }
    matches
}
