//! Chunking invariance: any split of a stream gives the same screen, cursor, title and events.

use std::time::Instant;

use oneterm_vt::{Config, EventBatch, Size, Terminal, VtEvent};
use proptest::prelude::*;

/// One element of a hostile-ish stream: control bytes and sequence pieces, text, a wide
/// character and a combining mark.
fn piece() -> impl Strategy<Value = Vec<u8>> {
    let bytes = |text: &'static str| text.as_bytes().to_vec();
    prop_oneof![
        8 => prop::sample::select(vec![
            bytes("\x1b"), bytes("["), bytes("]"), bytes("P"), bytes(";"), bytes("?"),
            bytes("m"), bytes("H"), bytes("J"), bytes("K"), bytes("r"), bytes("\x07"),
            bytes("\x18"), bytes("a"), bytes("中"), bytes("\u{301}"), bytes("\r"), bytes("\n"),
            bytes("b"), bytes("0"), bytes("1"), bytes("2"), bytes("4"), bytes("8"),
            bytes("\x1b[?2027h"), bytes("\x1b[4h"), bytes("\x1b[4l"),
            bytes("\x1b[2J"), bytes("\x1b]2;t\x07"), bytes("\x1b[c"),
            bytes("\x1bP$qm\x1b\\"), bytes("\x1b]8;;u\x07"),
        ]),
        1 => any::<u8>().prop_map(|byte| vec![byte]),
    ]
}

fn stream() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(piece(), 0..300).prop_map(|pieces| pieces.concat())
}

/// An event with its payload read out of the batch that holds it.
fn describe(event: &VtEvent, batch: &EventBatch) -> Option<String> {
    Some(match event {
        VtEvent::Repaint => return None,
        VtEvent::Title(span) => format!("Title({})", batch.str(*span)),
        VtEvent::IconName(span) => format!("IconName({})", batch.str(*span)),
        VtEvent::Reply(span) => format!("Reply({:?})", batch.bytes(*span)),
        VtEvent::ClipboardStore { selection, text } => {
            format!("ClipboardStore({selection:?}, {})", batch.str(*text))
        }
        VtEvent::Cwd { host, path } => {
            format!("Cwd({}, {})", batch.str(*host), batch.str(*path))
        }
        VtEvent::Notification { title, body } => {
            format!("Notification({}, {})", batch.str(*title), batch.str(*body))
        }
        VtEvent::Pointer(span) => format!("Pointer({})", batch.str(*span)),
        VtEvent::Osc {
            code,
            params,
            terminator,
            truncated,
        } => {
            let params: Vec<&[u8]> = batch.params(*params).collect();
            format!("Osc({code}, {params:?}, {terminator:?}, {truncated})")
        }
        other => format!("{other:?}"),
    })
}

fn state(term: &Terminal) -> (Vec<String>, String, Option<String>, u64) {
    let screen = term.screen();
    let rows = (screen.oldest().0..=screen.newest().0)
        .map(|id| term.row_text(oneterm_vt::RowId(id)))
        .collect();
    let cursor = format!("{:?} {}", screen.cursor().pos, screen.cursor().pending_wrap);
    (
        rows,
        cursor,
        term.title().map(str::to_owned),
        term.lines_produced(),
    )
}

fn new_terminal() -> Terminal {
    let config = Config {
        scrollback_limit: 50,
        ..Config::default()
    };
    Terminal::new(Size { rows: 6, cols: 12 }, config)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn chunking_is_invariant(bytes in stream(), sizes in prop::collection::vec(1usize..=64, 1..40)) {
        let mut whole = new_terminal();
        let mut batch = EventBatch::new();
        whole.feed(&bytes, &mut batch, Instant::now());
        let expected: Vec<String> = batch
            .iter()
            .filter_map(|event| describe(event, &batch))
            .collect();
        let whole_stats = whole.stats();

        let mut split = new_terminal();
        let mut events = Vec::new();
        let mut unhandled = 0u32;
        let mut rest = bytes.as_slice();
        let mut sizes = sizes.iter().cycle();
        while !rest.is_empty() {
            let take = (*sizes.next().unwrap_or(&1)).min(rest.len());
            let (chunk, tail) = rest.split_at(take);
            rest = tail;
            let stats = split.feed(chunk, &mut batch, Instant::now());
            unhandled = unhandled.saturating_add(stats.unhandled_sequences);
            events.extend(batch.iter().filter_map(|event| describe(event, &batch)));
        }

        prop_assert_eq!(state(&split), state(&whole));
        prop_assert_eq!(events, expected);
        prop_assert_eq!(unhandled, whole_stats.unhandled_sequences);
    }
}
