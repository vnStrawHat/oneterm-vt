use std::time::Instant;

use super::*;
use crate::{Config, EventBatch, Size};

type Hit = (i32, usize, i32, usize);

fn terminal(cols: u16, rows: u16, bytes: &[u8]) -> Terminal {
    let mut term = Terminal::new(Size { rows, cols }, Config::default());
    term.feed(bytes, &mut EventBatch::new(), Instant::now());
    term
}

fn options(case_sensitive: bool, whole_word: bool) -> SearchOptions {
    SearchOptions {
        case_sensitive,
        whole_word,
    }
}

fn find(term: &Terminal, needle: &str, options: SearchOptions) -> Vec<SearchMatch> {
    let text = GridText::from_terminal(term);
    search_grid_text(&text, SearchPattern::Literal(needle), options)
}

/// `(grid line of row, start col, grid line of end row, end col)` per match.
fn hits(term: &Terminal, matches: &[SearchMatch]) -> Vec<Hit> {
    let top = term.screen().screen_top();
    matches
        .iter()
        .map(|found| {
            let end_row = SearchMatch {
                row: found.end_row,
                ..*found
            };
            (
                found.grid_line(top),
                found.start_col,
                end_row.grid_line(top),
                found.end_col,
            )
        })
        .collect()
}

fn hits_of(term: &Terminal, needle: &str, options: SearchOptions) -> Vec<Hit> {
    hits(term, &find(term, needle, options))
}

fn default_hits(bytes: &str, cols: u16, needle: &str) -> Vec<Hit> {
    hits_of(
        &terminal(cols, 4, bytes.as_bytes()),
        needle,
        options(false, false),
    )
}

// Model and mapping.

#[test]
fn copy_is_keyed_by_row_id() {
    let term = terminal(6, 2, b"alpha\r\nbeta\r\nalpha");
    let text = GridText::from_terminal(&term);
    assert_eq!(text.oldest(), term.screen().oldest());
    assert_eq!(text.rows(), 3);
    let top = term.screen().screen_top();
    drop(term);
    let found = search_grid_text(
        &text,
        SearchPattern::Literal("alpha"),
        options(false, false),
    );
    let lines: Vec<i32> = found.iter().map(|m| m.grid_line(top)).collect();
    assert_eq!(lines, [-1, 1]);
}

#[test]
fn display_row_adds_the_offset() {
    let term = terminal(6, 2, b"alpha\r\nbeta\r\nalpha");
    let top = term.screen().screen_top();
    let found = find(&term, "alpha", options(false, false));
    assert_eq!(found[0].display_row(top, 1), 0);
    assert_eq!(found[1].display_row(top, usize::MAX), i32::MAX);
}

#[test]
fn grid_line_clamps() {
    let far = SearchMatch {
        row: RowId(u64::MAX),
        start_col: 0,
        end_row: RowId(u64::MAX),
        end_col: 0,
    };
    assert_eq!(far.grid_line(RowId(0)), i32::MAX);
    let below = SearchMatch {
        row: RowId(0),
        ..far
    };
    assert_eq!(below.grid_line(RowId(u64::MAX)), i32::MIN);
}

#[test]
fn alt_screen_is_searched_when_active() {
    let term = terminal(10, 3, b"main\x1b[?1049halt text");
    let found = find(&term, "alt", options(false, false));
    assert_eq!(found.len(), 1);
    assert!(found[0].row >= RowId::ALT_ORIGIN);
    assert!(find(&term, "main", options(false, false)).is_empty());
}

#[test]
fn blank_rows_and_unwritten_rows_copy_empty() {
    let term = terminal(5, 3, b"");
    let text = GridText::from_terminal(&term);
    assert_eq!(text.rows(), 3);
    let found = search_grid_text(&text, SearchPattern::Literal(" "), options(false, false));
    assert!(found.is_empty());
}

#[test]
fn grid_text_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<GridText>();
}

// Literal.

#[test]
fn empty_needle_finds_nothing() {
    assert!(default_hits("abc", 10, "").is_empty());
}

#[test]
fn newline_needle_finds_nothing() {
    assert!(default_hits("a\r\nb", 10, "a\nb").is_empty());
}

#[test]
fn capital_sharp_s_folds_to_sharp_s() {
    let term = terminal(10, 3, "ẞ".as_bytes());
    assert_eq!(find(&term, "ß", options(false, false)).len(), 1);
}

#[test]
fn default_is_case_insensitive() {
    assert_eq!(default_hits("Hello World", 20, "WORLD"), [(0, 6, 0, 11)]);
}

#[test]
fn case_sensitive_respects_case() {
    let term = terminal(20, 4, b"Hello World");
    assert!(hits_of(&term, "hello", options(true, false)).is_empty());
    assert_eq!(
        hits_of(&term, "Hello", options(true, false)),
        [(0, 0, 0, 5)]
    );
}

#[test]
fn matches_do_not_overlap() {
    assert_eq!(
        default_hits("aaaaa", 10, "aa"),
        [(0, 0, 0, 2), (0, 2, 0, 4)]
    );
}

#[test]
fn matches_run_top_to_bottom() {
    assert_eq!(
        default_hits("x foo\r\nfoo foo", 10, "foo"),
        [(0, 2, 0, 5), (1, 0, 1, 3), (1, 4, 1, 7)]
    );
}

#[test]
fn whole_word_excludes_substrings() {
    let term = terminal(20, 4, b"foo foobar foo");
    assert_eq!(
        hits_of(&term, "foo", options(false, true)),
        [(0, 0, 0, 3), (0, 11, 0, 14)]
    );
}

#[test]
fn whole_word_counts_underscore() {
    let term = terminal(20, 4, b"foo_bar foo");
    assert_eq!(hits_of(&term, "foo", options(false, true)), [(0, 8, 0, 11)]);
}

#[test]
fn rejected_hit_does_not_hide_the_next() {
    let term = terminal(20, 4, b"aa-a-a");
    assert_eq!(hits_of(&term, "a-a", options(false, true)), [(0, 3, 0, 6)]);
}

#[test]
fn wide_needle_spans_spacers() {
    assert_eq!(default_hits("日本語", 10, "本語"), [(0, 2, 0, 6)]);
    assert_eq!(default_hits("日本語", 10, "日"), [(0, 0, 0, 2)]);
}

#[test]
fn cluster_is_searched_whole() {
    // `?2027` places a whole cluster in one cell.
    let bytes = "\x1b[?2027h👍🏽";
    assert_eq!(default_hits(bytes, 10, "👍🏽"), [(0, 0, 0, 2)]);
    assert_eq!(default_hits(bytes, 10, "👍"), [(0, 0, 0, 2)]);
}

#[test]
fn cyrillic_folds() {
    assert_eq!(default_hits("ПРИВЕТ мир", 20, "привет"), [(0, 0, 0, 6)]);
}

#[test]
fn kelvin_sign_does_not_fold_to_ascii() {
    assert!(default_hits("\u{212A}", 10, "k").is_empty());
}

#[test]
fn unicode_letters_are_word_chars() {
    let term = terminal(20, 4, "fooé foo".as_bytes());
    assert_eq!(hits_of(&term, "foo", options(false, true)), [(0, 5, 0, 8)]);
}

#[test]
fn trailing_blanks_are_not_text() {
    assert!(default_hits("foo   \r\n", 10, "foo ").is_empty());
}

#[test]
fn tab_cell_is_a_tab() {
    assert_eq!(default_hits("a\tb", 20, "a\t"), [(0, 0, 0, 2)]);
}

// Across wrapped rows.

#[test]
fn match_spans_a_wrap() {
    assert_eq!(default_hits("0123456789abc", 10, "89ab"), [(0, 8, 1, 2)]);
}

#[test]
fn match_ends_on_the_last_column() {
    assert_eq!(default_hits("0123456789abc", 10, "789"), [(0, 7, 0, 10)]);
}

#[test]
fn match_spans_three_rows() {
    assert_eq!(default_hits("abcdefghij", 4, "bcdefghi"), [(0, 1, 2, 1)]);
}

#[test]
fn leading_spacer_is_skipped() {
    assert_eq!(default_hits("abcdefghi中", 10, "i中"), [(0, 8, 1, 2)]);
}

#[test]
fn hard_break_is_not_joined() {
    assert!(default_hits("a\r\nb", 10, "ab").is_empty());
    assert!(default_hits("a\nb", 10, "ab").is_empty());
}

#[test]
fn wrap_join_survives_scrollback() {
    let term = terminal(5, 2, b"0123456789xyz");
    assert_eq!(
        hits_of(&term, "34567", options(false, false)),
        [(-1, 3, 0, 3)]
    );
}

// Limits and hostile rows.

fn hostile_line() -> Terminal {
    terminal(2000, 500, "a".repeat(1_000_000).as_bytes())
}

#[test]
fn one_hostile_logical_line() {
    let term = hostile_line();
    assert!(find(&term, "aaab", options(false, false)).is_empty());
    assert!(find(&term, "a", options(false, true)).is_empty());
}

#[test]
fn match_count_is_capped() {
    let term = hostile_line();
    let found = find(&term, "a", options(false, false));
    assert_eq!(found.len(), MAX_MATCHES);
    assert!(found[0].row < found[MAX_MATCHES - 1].row);
}

#[test]
fn cluster_heavy_row_maps_columns() {
    let cluster = format!("e{}", "\u{301}".repeat(15));
    let bytes = format!("\x1b[?2027h{}", cluster.repeat(20));
    let term = terminal(20, 1, bytes.as_bytes());
    let found = hits_of(&term, &cluster, options(true, false));
    let expected: Vec<Hit> = (0..20).map(|col| (0, col, 0, col + 1)).collect();
    assert_eq!(found, expected);
}

#[test]
fn ascii_rows_store_no_column_table() {
    let text = GridText::from_terminal(&terminal(10, 3, b"abc\r\ndef"));
    assert!(text.columns.is_empty());
}

// Golden literal table: runs in every build, so a feature that changes literal results fails.

struct Golden {
    bytes: &'static str,
    cols: u16,
    needle: &'static str,
    case_sensitive: bool,
    whole_word: bool,
    expected: &'static [Hit],
}

const GOLDEN: &[Golden] = &[
    Golden {
        bytes: "Hello World",
        cols: 20,
        needle: "world",
        case_sensitive: false,
        whole_word: false,
        expected: &[(0, 6, 0, 11)],
    },
    Golden {
        bytes: "Hello World",
        cols: 20,
        needle: "hello",
        case_sensitive: true,
        whole_word: false,
        expected: &[],
    },
    Golden {
        bytes: "foo bar foo baz foo",
        cols: 30,
        needle: "foo",
        case_sensitive: false,
        whole_word: false,
        expected: &[(0, 0, 0, 3), (0, 8, 0, 11), (0, 16, 0, 19)],
    },
    Golden {
        bytes: "foo foobar foo",
        cols: 20,
        needle: "foo",
        case_sensitive: false,
        whole_word: true,
        expected: &[(0, 0, 0, 3), (0, 11, 0, 14)],
    },
    Golden {
        bytes: "foo_bar foo",
        cols: 20,
        needle: "foo",
        case_sensitive: false,
        whole_word: true,
        expected: &[(0, 8, 0, 11)],
    },
    Golden {
        bytes: "aa-a-a",
        cols: 20,
        needle: "a-a",
        case_sensitive: false,
        whole_word: true,
        expected: &[(0, 3, 0, 6)],
    },
    Golden {
        bytes: "日本語",
        cols: 20,
        needle: "本語",
        case_sensitive: false,
        whole_word: false,
        expected: &[(0, 2, 0, 6)],
    },
    Golden {
        bytes: "ПРИВЕТ мир",
        cols: 20,
        needle: "привет",
        case_sensitive: false,
        whole_word: false,
        expected: &[(0, 0, 0, 6)],
    },
    Golden {
        bytes: "0123456789abc",
        cols: 10,
        needle: "89ab",
        case_sensitive: false,
        whole_word: false,
        expected: &[(0, 8, 1, 2)],
    },
    Golden {
        bytes: "abcdefghi中",
        cols: 10,
        needle: "i中",
        case_sensitive: false,
        whole_word: false,
        expected: &[(0, 8, 1, 2)],
    },
    Golden {
        bytes: "a\r\nb",
        cols: 20,
        needle: "ab",
        case_sensitive: false,
        whole_word: false,
        expected: &[],
    },
];

#[test]
fn literal_golden_table() {
    for case in GOLDEN {
        let term = terminal(case.cols, 4, case.bytes.as_bytes());
        let found = hits_of(
            &term,
            case.needle,
            options(case.case_sensitive, case.whole_word),
        );
        assert_eq!(
            found, case.expected,
            "{:?} for {:?}",
            case.needle, case.bytes
        );
    }
}

#[cfg(feature = "regex")]
mod regex_pattern {
    use ::regex::{Regex, escape};

    use super::*;

    fn regex_hits(term: &Terminal, pattern: &str) -> Vec<SearchMatch> {
        let regex = Regex::new(pattern).unwrap();
        let text = GridText::from_terminal(term);
        search_grid_text(
            &text,
            SearchPattern::Regex(&regex),
            SearchOptions::default(),
        )
    }

    fn regex_hit_list(bytes: &str, cols: u16, pattern: &str) -> Vec<Hit> {
        let term = terminal(cols, 4, bytes.as_bytes());
        hits(&term, &regex_hits(&term, pattern))
    }

    #[test]
    fn escaped_needle_matches_the_literal() {
        for case in GOLDEN.iter().filter(|case| !case.whole_word) {
            let term = terminal(case.cols, 4, case.bytes.as_bytes());
            let flag = if case.case_sensitive { "" } else { "(?i)" };
            let pattern = format!("{flag}{}", escape(case.needle));
            let literal = find(&term, case.needle, options(case.case_sensitive, false));
            assert_eq!(regex_hits(&term, &pattern), literal, "{:?}", case.needle);
        }
    }

    #[test]
    fn caret_anchors_at_line_start() {
        assert_eq!(regex_hit_list("foo foo", 20, "^foo"), [(0, 0, 0, 3)]);
    }

    #[test]
    fn caret_skips_the_second_row_of_a_wrapped_line() {
        assert!(regex_hit_list("0123456789foo", 10, "^foo").is_empty());
    }

    #[test]
    fn dollar_anchors_at_content_end() {
        let term = terminal(5, 1, b"ab");
        assert_eq!(hits(&term, &regex_hits(&term, "b$")), [(0, 1, 0, 2)]);
    }

    #[test]
    fn regex_spans_a_wrap() {
        assert_eq!(regex_hit_list("0123456789abc", 10, "9a"), [(0, 9, 1, 1)]);
    }

    #[test]
    fn empty_matching_pattern_terminates() {
        let term = terminal(2, 1, b"ab");
        let found = regex_hits(&term, "x*");
        let starts: Vec<usize> = found.iter().map(|m| m.start_col).collect();
        assert_eq!(starts, [0, 1, 2]);
        assert!(
            found
                .iter()
                .all(|m| m.end_col == m.start_col && m.end_row == m.row)
        );
    }

    #[test]
    fn options_are_ignored_for_regex() {
        let term = terminal(20, 4, b"Foo foo");
        let regex = Regex::new("foo").unwrap();
        let text = GridText::from_terminal(&term);
        let strict = options(true, true);
        let with_options = search_grid_text(&text, SearchPattern::Regex(&regex), strict);
        assert_eq!(with_options, regex_hits(&term, "foo"));
    }

    #[test]
    fn regex_match_count_is_capped() {
        let term = hostile_line();
        assert_eq!(regex_hits(&term, "a").len(), MAX_MATCHES);
    }
}
