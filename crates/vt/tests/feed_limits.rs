//! Hostile streams: no panic, every ceiling holds, every drop is counted.
//!
//! Capacities are read through the public `EventBatch` accessors. In debug builds every feed also
//! runs the grid's integrity walk, so a corrupt grid fails these tests too.

use std::time::Instant;

use oneterm_vt::parser::{DCS_MAX_BYTES, MAX_OSC_PARAMS, OSC_INLINE, OSC_LARGE};
use oneterm_vt::{Config, EventBatch, FeedStats, OscRoute, Size, Terminal, VtEvent};

const MIB: usize = 1024 * 1024;
const BATCH_SOFT: usize = MIB;

fn terminal(cols: u16, rows: u16, config: Config) -> Terminal {
    Terminal::new(Size { rows, cols }, config)
}

fn feed(term: &mut Terminal, batch: &mut EventBatch, bytes: &[u8]) -> FeedStats {
    term.feed(bytes, batch, Instant::now())
}

/// A tiny xorshift generator, so the streams are the same on every run.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn random_bytes_never_panic() {
    const SPECIAL: &[u8] = b"\x1b[]P;?0123456789\x07\x18m";
    for seed in [
        0x9E37_79B9_7F4A_7C15,
        0xD1B5_4A32_D192_ED03,
        0x1234_5678_9ABC_DEF1,
    ] {
        let mut rng = Xorshift(seed);
        let mut term = terminal(80, 24, Config::default());
        let mut batch = EventBatch::new();
        let stream: Vec<u8> = (0..MIB)
            .map(|_| {
                let word = rng.next();
                if word % 10 < 7 {
                    SPECIAL[(word >> 8) as usize % SPECIAL.len()]
                } else {
                    (word >> 16) as u8
                }
            })
            .collect();
        for chunk in stream.chunks(4096) {
            feed(&mut term, &mut batch, chunk);
        }
        // The terminal still works afterwards.
        feed(&mut term, &mut batch, b"\x1bc\x1b[1;1Hok");
        assert!(
            term.row_text(term.screen().row_of_index(0))
                .starts_with("ok")
        );
    }
}

#[test]
fn a_16_mib_clipboard_store_is_cut_at_8_mib_and_released() {
    let mut config = Config::default();
    config.osc_routes.large(52, true);
    let mut term = terminal(80, 24, config);
    let mut batch = EventBatch::new();
    let mut stream = b"\x1b]52;c;".to_vec();
    stream.extend(std::iter::repeat_n(b'A', 16 * MIB));
    stream.push(0x07);
    let stats = feed(&mut term, &mut batch, &stream);
    assert_eq!(stats.truncated_osc, 1);
    let stored = batch.iter().find_map(|event| match event {
        VtEvent::ClipboardStore { text, .. } => Some(batch.str(*text).len()),
        _ => None,
    });
    if let Some(len) = stored {
        assert!(len <= OSC_LARGE, "{len}");
    }
    feed(&mut term, &mut batch, b"a");
    assert!(batch.arena_capacity() <= BATCH_SOFT);
}

#[test]
fn a_forwarded_flood_of_separators_is_bounded_with_large() {
    let mut config = Config::default();
    config
        .osc_routes
        .route(7501, OscRoute::Forward)
        .large(7501, true);
    let mut term = terminal(80, 24, config);
    let mut batch = EventBatch::new();
    let mut stream = b"\x1b]7501;".to_vec();
    stream.extend(std::iter::repeat_n(b';', 8 * MIB));
    stream.push(0x07);
    let stats = feed(&mut term, &mut batch, &stream);
    let osc: Vec<_> = batch
        .iter()
        .filter_map(|event| match event {
            VtEvent::Osc { params, .. } => Some(batch.params(*params).count()),
            _ => None,
        })
        .collect();
    assert_eq!(osc.len(), 1);
    // The 15 separators before the last parameter are not payload bytes: they add 15 spans.
    assert!(osc[0] <= OSC_LARGE + MAX_OSC_PARAMS, "{}", osc[0]);
    assert!(osc[0] > OSC_INLINE);
    assert_eq!(stats.truncated_osc, 0);
    feed(&mut term, &mut batch, b"a");
    assert!(batch.param_capacity() <= 4096);
}

#[test]
fn a_forwarded_flood_of_separators_is_cut_at_2_kib_without_large() {
    let mut config = Config::default();
    config.osc_routes.route(7501, OscRoute::Forward);
    let mut term = terminal(80, 24, config);
    let mut batch = EventBatch::new();
    let mut stream = b"\x1b]7501;".to_vec();
    stream.extend(std::iter::repeat_n(b';', 8 * MIB));
    stream.push(0x07);
    let stats = feed(&mut term, &mut batch, &stream);
    let counts: Vec<usize> = batch
        .iter()
        .filter_map(|event| match event {
            VtEvent::Osc { params, .. } => Some(batch.params(*params).count()),
            _ => None,
        })
        .collect();
    assert_eq!(counts.len(), 1);
    // The 15 separators before the last parameter are not payload bytes: they add 15 spans.
    assert!(counts[0] <= OSC_INLINE + MAX_OSC_PARAMS, "{}", counts[0]);
    assert_eq!(stats.truncated_osc, 1);
}

#[test]
fn title_pushes_are_capped_at_sixteen() {
    let mut term = terminal(80, 24, Config::default());
    let mut batch = EventBatch::new();
    feed(&mut term, &mut batch, &b"\x1b[22t".repeat(100_000));
    assert_eq!(term.title_depth(), 16);
}

#[test]
fn keyboard_flag_pushes_are_capped_and_a_huge_pop_empties() {
    let mut term = terminal(80, 24, Config::default());
    let mut batch = EventBatch::new();
    feed(&mut term, &mut batch, &b"\x1b[>1u".repeat(100_000));
    feed(&mut term, &mut batch, b"\x1b[<65535u");
    assert!(term.keyboard_flags().is_empty());
    feed(&mut term, &mut batch, b"\x1b[?u");
    let VtEvent::Reply(span) = batch.events()[0] else {
        panic!("a reply");
    };
    assert_eq!(batch.bytes(span), b"\x1b[?0u");
}

#[test]
fn termcap_requests_answer_sixteen_names() {
    let mut term = terminal(80, 24, Config::default());
    let mut batch = EventBatch::new();
    let mut stream = b"\x1bP+q".to_vec();
    stream.extend(std::iter::repeat_n(b';', 4000));
    stream.extend_from_slice(b"\x1b\\");
    let stats = feed(&mut term, &mut batch, &stream);
    let replies = batch
        .iter()
        .filter(|event| matches!(event, VtEvent::Reply(_)))
        .count();
    assert_eq!(replies, 16);
    assert_eq!(stats.unhandled_sequences, 1);
}

#[test]
fn an_oversized_status_request_is_dropped() {
    let mut term = terminal(80, 24, Config::default());
    let mut batch = EventBatch::new();
    let mut stream = b"\x1bP$q".to_vec();
    stream.extend(std::iter::repeat_n(b'm', 9 * 1024));
    stream.extend_from_slice(b"\x1b\\");
    let stats = feed(&mut term, &mut batch, &stream);
    assert!(!batch.iter().any(|event| matches!(event, VtEvent::Reply(_))));
    assert_eq!((stats.unhandled_sequences, stats.aborted_dcs), (1, 0));
}

#[test]
fn a_dcs_past_its_cap_is_aborted_without_an_answer() {
    let mut term = terminal(80, 24, Config::default());
    let mut batch = EventBatch::new();
    let mut stream = b"\x1bP$q".to_vec();
    stream.extend(std::iter::repeat_n(b'm', DCS_MAX_BYTES + 10));
    stream.extend_from_slice(b"\x1b\\");
    let stats = feed(&mut term, &mut batch, &stream);
    assert_eq!(stats.aborted_dcs, 1);
    assert!(
        batch
            .iter()
            .all(|event| !matches!(event, VtEvent::Reply(_)))
    );
}

#[test]
fn seventy_thousand_links_exhaust_the_table_but_every_character_prints() {
    let config = Config {
        scrollback_limit: 10_000,
        ..Config::default()
    };
    let mut term = terminal(160, 45, config);
    let mut batch = EventBatch::new();
    let mut stream = Vec::new();
    for link in 0..70_000u32 {
        stream.extend_from_slice(format!("\x1b]8;;http://l/{link}\x07x").as_bytes());
    }
    let stats = feed(&mut term, &mut batch, &stream);
    assert!(stats.hyperlink_table_exhausted > 0);
    let screen = term.screen();
    let printed: usize = (screen.oldest().0..=screen.newest().0)
        .map(|id| {
            term.row_text(oneterm_vt::RowId(id))
                .chars()
                .filter(|c| *c == 'x')
                .count()
        })
        .sum();
    assert_eq!(printed, 70_000);
}

#[test]
fn three_hundred_thousand_clusters_are_swept_not_exhausted() {
    let config = Config {
        scrollback_limit: 100,
        ..Config::default()
    };
    let mut term = terminal(80, 24, config);
    let mut batch = EventBatch::new();
    let stream: String = (0..300_000u32)
        .map(|i| {
            let base = char::from_u32(0x4E00 + i % 20_000).unwrap_or('x');
            let mark = char::from_u32(0x300 + i / 20_000).unwrap_or('\u{301}');
            format!("{base}{mark}")
        })
        .collect();
    let mut exhausted = 0u32;
    let (mut previous, mut swept) = (0, false);
    for chunk in stream.as_bytes().chunks(64 * 1024) {
        exhausted += feed(&mut term, &mut batch, chunk).grapheme_table_exhausted;
        let entries = term.interner().graphemes.entries();
        assert!(entries <= 1 << 18);
        swept |= entries < previous;
        previous = entries;
    }
    assert!(swept, "the arena was swept at least once");
    assert_eq!(exhausted, 0);
}

#[test]
fn a_hundred_thousand_bells_in_one_feed_do_not_pin_the_event_list() {
    let mut term = terminal(80, 24, Config::default());
    let mut batch = EventBatch::new();
    feed(&mut term, &mut batch, &vec![0x07; 100_000]);
    assert_eq!(batch.len(), 100_001);
    assert_eq!(batch.events()[100_000], VtEvent::Repaint);
    assert!(
        batch.events()[..100_000]
            .iter()
            .all(|event| *event == VtEvent::Bell)
    );
    feed(&mut term, &mut batch, b"a");
    assert!(batch.event_capacity() <= 4096);
}

#[test]
fn repeat_floods_stay_inside_the_scrollback() {
    let mut term = terminal(80, 24, Config::default());
    let mut batch = EventBatch::new();
    feed(&mut term, &mut batch, b"x");
    let mut prints = 0usize;
    for _ in 0..10 {
        let stats = feed(&mut term, &mut batch, b"\x1b[65535b");
        prints += 65_535;
        assert!(stats.rows_scrolled > 0);
    }
    assert_eq!(prints, 655_350);
    assert!(term.screen().history_len() <= 10_000);
}

#[test]
fn a_cluster_that_never_ends_is_dropped_once() {
    let mut term = terminal(80, 24, Config::default());
    let mut batch = EventBatch::new();
    feed(&mut term, &mut batch, b"\x1b[?2027ha");
    let mut dropped = 0;
    for _ in 0..20_000 {
        dropped += feed(&mut term, &mut batch, "\u{301}".as_bytes()).dropped_cluster_carries;
    }
    assert_eq!(dropped, 1);
}

#[test]
fn ten_thousand_of_every_query_answer_in_order_and_release_the_arena() {
    const QUERIES: &[(&[u8], &[&[u8]])] = &[
        (b"\x1b[c", &[b"\x1b[?62;4;22c"]),
        (b"\x1b[>c", &[b"\x1b[>0;100;1c"]),
        (b"\x1b[=c", &[b"\x1bP!|00000000\x1b\\"]),
        (b"\x1b[5n", &[b"\x1b[0n"]),
        (b"\x1b[6n", &[b"\x1b[1;1R"]),
        (b"\x1b[?6n", &[b"\x1b[?1;1;1R"]),
        (b"\x1b[4$p", &[b"\x1b[4;2$y"]),
        (b"\x1b[?7$p", &[b"\x1b[?7;1$y"]),
        (b"\x1b[?u", &[b"\x1b[?0u"]),
        (b"\x1b[?4m", &[b"\x1b[>4;0m"]),
        (b"\x1b[14t", &[b"\x1b[4;0;0t"]),
        (b"\x1b[18t", &[b"\x1b[8;24;80t"]),
        (b"\x1bP$qm\x1b\\", &[b"\x1bP1$r0m\x1b\\"]),
        (
            b"\x1bP+q544e\x1b\\",
            &[b"\x1bP1+r544e=787465726D2D323536636F6C6F72\x1b\\"],
        ),
        (
            b"\x1bP+q436f;524742\x1b\\",
            &[b"\x1bP1+r436f=323536\x1b\\", b"\x1bP1+r524742=\x1b\\"],
        ),
    ];
    let round: Vec<u8> = QUERIES
        .iter()
        .flat_map(|(query, _)| query.iter().copied())
        .collect();
    let expected: Vec<&[u8]> = QUERIES
        .iter()
        .flat_map(|(_, replies)| replies.iter().copied())
        .collect();
    let mut term = terminal(80, 24, Config::default());
    let mut batch = EventBatch::new();
    feed(&mut term, &mut batch, &round.repeat(10_000));
    let replies: Vec<&[u8]> = batch
        .iter()
        .filter_map(|event| match event {
            VtEvent::Reply(span) => Some(batch.bytes(*span)),
            _ => None,
        })
        .collect();
    assert_eq!(replies.len(), 10_000 * expected.len());
    for (index, reply) in replies.iter().enumerate() {
        assert_eq!(*reply, expected[index % expected.len()], "reply {index}");
    }
    assert!(batch.arena_capacity() > BATCH_SOFT);
    feed(&mut term, &mut batch, b"a");
    assert!(batch.arena_capacity() <= BATCH_SOFT);
}
