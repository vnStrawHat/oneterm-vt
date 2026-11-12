//! How many columns text takes.
//!
//! Two functions, because a terminal measures in one of two ways. Without grapheme
//! clustering (the power-on default) each scalar is measured with [`scalar_width`]. With
//! clustering (mode `?2027`) the text is split into clusters and each is measured with
//! [`cluster_width`], so a whole cluster takes one cell. Both are pure, total, never panic,
//! and never return more than 2.
//!
//! The results depend on the Unicode tables of the exact `unicode-width` and
//! `unicode-segmentation` versions in use; the crate pins `unicode-width`, so a table change
//! is always a deliberate upgrade.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Returns the columns `c` takes on its own: 0, 1 or 2.
///
/// Controls (including NUL, DEL and C1), combining marks, zero-width joiners, variation
/// selectors and conjoining Hangul vowels and final consonants are 0 and attach to the
/// previous cell. East Asian Wide and Fullwidth scalars, and scalars with emoji
/// presentation, are 2. Ambiguous-width, unassigned and private-use scalars are 1 (a terminal
/// built with `Config::ambiguous_wide` measures the ambiguous ones as 2 itself).
pub fn scalar_width(c: char) -> u8 {
    // Clamped to 2: a few scalars are rated 3.
    UnicodeWidthChar::width(c).unwrap_or(0).min(2) as u8
}

/// Returns the columns the first extended grapheme cluster of `text` takes: 0, 1 or 2.
///
/// Empty text, and a cluster starting with a control (such as `"\r\n"`), measure 0. Emoji
/// ZWJ sequences, skin-tone sequences, flags and `base + VS16` are 2; `VS15` on an emoji
/// base is 1. Text after the first cluster is ignored.
pub fn cluster_width(text: &str) -> u8 {
    text.graphemes(true).next().map_or(0, cluster_str_width)
}

/// The width of `cluster`, which the caller has already cut out as one grapheme cluster: 0 for
/// a cluster that starts with a control, else its string width clamped to 2 columns.
pub(crate) fn cluster_str_width(cluster: &str) -> u8 {
    if cluster.chars().next().is_some_and(char::is_control) {
        return 0;
    }
    // Clamped to 2: string rules sum the parts of unqualified sequences.
    UnicodeWidthStr::width(cluster).min(2) as u8
}

/// [`scalar_width`] for a terminal that draws East Asian Ambiguous scalars two columns wide
/// (`Config::ambiguous_wide`), as CJK legacy fonts do.
pub(crate) fn scalar_width_cjk(c: char) -> u8 {
    // Clamped to 2: a few scalars are rated 3.
    UnicodeWidthChar::width_cjk(c).unwrap_or(0).min(2) as u8
}

/// [`cluster_str_width`] with East Asian Ambiguous scalars two columns wide.
pub(crate) fn cluster_str_width_cjk(cluster: &str) -> u8 {
    if cluster.chars().next().is_some_and(char::is_control) {
        return 0;
    }
    // Clamped to 2: string rules sum the parts of unqualified sequences.
    UnicodeWidthStr::width_cjk(cluster).min(2) as u8
}

#[cfg(test)]
#[path = "width_tests.rs"]
mod width_tests;
