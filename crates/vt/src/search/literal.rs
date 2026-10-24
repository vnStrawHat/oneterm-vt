//! The literal matcher: one left-to-right scan of the copied text.

use memchr::memmem::Finder;
use memchr::{memchr, memchr2};

use super::{GridText, MAX_MATCHES, SearchMatch, SearchOptions};

/// How a candidate hit is found and checked.
enum Needle<'a> {
    /// Byte-exact; a UTF-8 needle in UTF-8 text always hits on char boundaries.
    Exact(Box<Finder<'a>>),
    /// The needle folded char by char; the text is folded as it is compared.
    Folded(Vec<char>),
}

impl Needle<'_> {
    /// The next hit at or after byte `from`, as `(start, end)`.
    fn next_hit(&self, text: &str, from: usize) -> Option<(usize, usize)> {
        match self {
            Needle::Exact(finder) => {
                let found = from + finder.find(text.as_bytes().get(from..)?)?;
                Some((found, found + finder.needle().len()))
            }
            Needle::Folded(folded) => next_folded_hit(text, from, folded),
        }
    }
}

/// Finds every hit of `needle`, top to bottom, without overlap, at most `MAX_MATCHES`.
pub(super) fn search(text: &GridText, needle: &str, options: SearchOptions) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    // A match never crosses a hard line end, so a needle holding one finds nothing.
    if needle.is_empty() || needle.contains('\n') {
        return matches;
    }
    let needle = if options.case_sensitive {
        Needle::Exact(Box::new(Finder::new(needle.as_bytes())))
    } else {
        Needle::Folded(needle.chars().map(fold).collect())
    };
    let haystack = text.text.as_str();
    let mut from = 0;
    while let Some((start, end)) = needle.next_hit(haystack, from) {
        if options.whole_word
            && !(is_word_boundary(haystack, start) && is_word_boundary(haystack, end))
        {
            // A rejected hit may overlap a valid one that starts inside it.
            from = start + char_len_at(haystack, start);
            continue;
        }
        let Some(found) = text.span(start, end) else {
            break;
        };
        matches.push(found);
        if matches.len() == MAX_MATCHES {
            break;
        }
        from = end;
    }
    matches
}

/// Folds one char to lower case, 1:1 and never into ASCII from non-ASCII.
///
/// The one-char length keeps columns exact; the ASCII rule makes the byte prefilter in
/// `next_candidate` exact: no non-ASCII char folds to an ASCII letter (U+212A KELVIN SIGN stays
/// itself). This is not Unicode case folding.
fn fold(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(single), None) if !single.is_ascii() => single,
        _ => c,
    }
}

/// Whether `at` sits between two chars that are not both word chars.
fn is_word_boundary(text: &str, at: usize) -> bool {
    let before = text.get(..at).and_then(|head| head.chars().next_back());
    let after = text.get(at..).and_then(|tail| tail.chars().next());
    !(before.is_some_and(is_word_char) && after.is_some_and(is_word_char))
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn char_len_at(text: &str, at: usize) -> usize {
    text.get(at..)
        .and_then(|tail| tail.chars().next())
        .map_or(1, char::len_utf8)
}

// ponytail: worst case O(text x needle) on self-similar text (`aaaa...` vs `aaa...b`); the needle
// is the user's typing, not program output. Upgrade to KMP over folded chars if a profile asks.
fn next_folded_hit(text: &str, from: usize, folded: &[char]) -> Option<(usize, usize)> {
    let first = *folded.first()?;
    let mut from = from;
    loop {
        let start = next_candidate(text, from, first)?;
        if let Some(end) = verify_folded(text, start, folded) {
            return Some((start, end));
        }
        from = start + char_len_at(text, start);
    }
}

/// The next char at or after `from` that folds to `first`.
fn next_candidate(text: &str, from: usize, first: char) -> Option<usize> {
    let tail = text.get(from..)?;
    let Some(byte) = u8::try_from(first).ok().filter(u8::is_ascii) else {
        // `first` is non-ASCII: only a non-ASCII char folds to it.
        return tail
            .char_indices()
            .find(|&(_, c)| fold(c) == first)
            .map(|(offset, _)| from + offset);
    };
    let offset = if byte.is_ascii_lowercase() {
        memchr2(byte, byte.to_ascii_uppercase(), tail.as_bytes())
    } else {
        memchr(byte, tail.as_bytes())
    }?;
    Some(from + offset)
}

/// The end of the hit starting at `start` if every folded char matches. A `'\n'` in the text
/// never equals a needle char, so a hit cannot cross a line end.
fn verify_folded(text: &str, start: usize, folded: &[char]) -> Option<usize> {
    let tail = text.get(start..)?;
    let mut chars = tail.char_indices();
    for &wanted in folded {
        let (_, c) = chars.next()?;
        if fold(c) != wanted {
            return None;
        }
    }
    Some(start + chars.next().map_or(tail.len(), |(offset, _)| offset))
}
