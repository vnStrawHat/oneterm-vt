use serde_json::Value;

use super::*;

fn plain(text: &str) -> Cell<'_> {
    Cell {
        text,
        ..Cell::default()
    }
}

fn dump_of(cols: u16, wants_history: bool, fill: impl FnOnce(&mut DumpBuilder)) -> Dump {
    let mut builder = DumpBuilder::new(cols, wants_history);
    fill(&mut builder);
    builder.finish()
}

#[test]
fn wide_char_and_spacer_give_the_char_once_and_blanks_one_space() {
    let dump = dump_of(6, false, |b| {
        b.row(|r| {
            r.cell(Cell {
                text: "国",
                wide: true,
                ..Cell::default()
            });
            r.cell(Cell {
                spacer: true,
                ..Cell::default()
            });
            r.cell(plain("a"));
            r.cell(plain(""));
            r.cell(plain("b"));
            r.cell(plain(""));
        });
    });
    // Empty cell inside the line is one space; the trailing blank is trimmed.
    assert_eq!(dump.rows, ["国a b"]);
}

#[test]
fn pending_wrap_cursor_clamps_to_the_last_column() {
    let dump = dump_of(160, false, |b| b.cursor(3, 160));
    assert_eq!(dump.cursor, (3, 159));
    let dump = dump_of(160, false, |b| b.cursor(3, 12));
    assert_eq!(dump.cursor, (3, 12));
}

fn styled(text: &str, fg: Color, flags: Flags) -> Cell<'_> {
    Cell {
        text,
        fg,
        flags,
        ..Cell::default()
    }
}

#[test]
fn runs_merge_equal_neighbours_skip_defaults_and_spacers_follow_their_lead() {
    let bold = Flags {
        bold: true,
        ..Flags::default()
    };
    let red = Color::Indexed(1);
    let dump = dump_of(8, false, |b| {
        b.row(|r| {
            r.cell(plain("p"));
            r.cell(styled("a", red, bold));
            r.cell(Cell {
                wide: true,
                ..styled("国", red, bold)
            });
            r.cell(Cell {
                spacer: true,
                ..Cell::default()
            });
            r.cell(styled("b", red, Flags::default()));
            r.cell(plain("c"));
            r.cell(plain(""));
            r.cell(styled("d", red, Flags::default()));
        });
    });
    assert_eq!(
        dump.attrs,
        [
            (0, 1, 4, "i1".into(), "d".into(), "b".into()),
            (0, 4, 5, "i1".into(), "d".into(), String::new()),
            (0, 7, 8, "i1".into(), "d".into(), String::new()),
        ]
    );
}

#[test]
fn colours_and_flags_use_the_dump_names() {
    let all = Flags {
        bold: true,
        italic: true,
        underline: true,
        inverse: true,
    };
    let dump = dump_of(3, false, |b| {
        b.row(|r| {
            r.cell(Cell {
                fg: Color::Indexed(1),
                bg: Color::Indexed(9),
                ..plain("a")
            });
            r.cell(Cell {
                fg: Color::Rgb(0x0a, 0xff, 0x00),
                flags: all,
                ..plain("b")
            });
        });
    });
    assert_eq!(dump.attrs[0].3, "i1");
    assert_eq!(dump.attrs[0].4, "i9");
    assert_eq!(dump.attrs[1].3, "r0aff00");
    assert_eq!(dump.attrs[1].5, "biru");
}

#[test]
fn links_are_runs_of_equal_uri_and_spacers_extend_them() {
    let link = |text, uri| Cell {
        uri: Some(uri),
        ..plain(text)
    };
    let dump = dump_of(6, false, |b| {
        b.row(|r| {
            r.cell(link("a", "u1"));
            r.cell(Cell {
                wide: true,
                ..link("国", "u1")
            });
            r.cell(Cell {
                spacer: true,
                ..Cell::default()
            });
            r.cell(link("b", "u2"));
            r.cell(plain("c"));
        });
    });
    assert_eq!(dump.links, [(0, 0, 3, "u1".into()), (0, 3, 4, "u2".into())]);
}

#[test]
fn a_base_char_with_marks_beside_it_is_one_cell_text() {
    let dump = dump_of(3, false, |b| {
        b.row(|r| {
            r.cell(plain("e\u{301}\u{323}"));
            r.cell(plain("x"));
        });
    });
    assert_eq!(dump.rows, ["e\u{301}\u{323}x"]);
}

#[test]
fn history_is_kept_only_when_wanted() {
    let fill = |b: &mut DumpBuilder| {
        b.history_row(|r| r.cell(plain("old")));
        b.row(|r| r.cell(plain("new")));
    };
    assert_eq!(dump_of(4, true, fill).history, ["old"]);
    assert!(dump_of(4, false, fill).history.is_empty());
}

fn color(name: &str) -> Color {
    match name.as_bytes() {
        [b'd'] => Color::Default,
        [b'i', rest @ ..] => Color::Indexed(std::str::from_utf8(rest).unwrap().parse().unwrap()),
        [b'r', rest @ ..] => {
            let hex = u32::from_str_radix(std::str::from_utf8(rest).unwrap(), 16).unwrap();
            Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
        }
        _ => panic!("colour {name}"),
    }
}

fn flags(letters: &str) -> Flags {
    Flags {
        bold: letters.contains('b'),
        italic: letters.contains('i'),
        underline: letters.contains('u'),
        inverse: letters.contains('r'),
    }
}

fn push_cells(row: &mut RowCells, cells: &Value) {
    for cell in cells.as_array().unwrap() {
        let cell = cell.as_array().unwrap();
        row.cell(Cell {
            text: cell[0].as_str().unwrap(),
            wide: cell[1] == 1,
            spacer: cell[2] == 1,
            fg: color(cell[3].as_str().unwrap()),
            bg: color(cell[4].as_str().unwrap()),
            flags: flags(cell[5].as_str().unwrap()),
            uri: cell[6].as_str(),
        });
    }
}

/// The Rust half of X1: `node adapter.mjs selftest` builds the same cases in JS.
const CASES: &str = include_str!("../tests/dump_cases.json");

fn case_dump(case: &Value) -> Dump {
    let cols = case["cols"].as_u64().unwrap() as u16;
    let mut builder = DumpBuilder::new(cols, case["history"].as_bool().unwrap());
    let cursor = case["cursor"].as_array().unwrap();
    builder.cursor(
        cursor[0].as_u64().unwrap() as usize,
        cursor[1].as_u64().unwrap() as usize,
    );
    for cells in case["history_rows"].as_array().unwrap() {
        builder.history_row(|row| push_cells(row, cells));
    }
    for cells in case["rows"].as_array().unwrap() {
        builder.row(|row| push_cells(row, cells));
    }
    builder.finish()
}

#[test]
fn shared_cases_produce_the_pinned_canonical_json() {
    let cases: Value = serde_json::from_str(CASES).unwrap();
    for case in cases.as_array().unwrap() {
        let got = case_dump(case).canonical_json();
        assert_eq!(got, case["expected"].as_str().unwrap(), "{}", case["name"]);
    }
}

#[test]
fn canonical_json_is_byte_stable_with_a_pinned_hash() {
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let first = case_dump(&cases[0]);
    assert!(
        first
            .canonical_json()
            .starts_with("{\"cursor\":[0,3],\"rows\":[")
    );
    assert_eq!(
        first.sha256(),
        "095b3f1a1cbfee52b41e019574edb0424b6afcf148d974f38a0411e59d22d480",
        "{}",
        first.canonical_json()
    );
}
