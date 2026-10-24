//! Byte-exact replies: one test per row of the reply table, through the public API only.
//!
//! These bytes are API; a change here is a major version bump.

use std::time::Instant;

use oneterm_vt::{
    ColorKey, Config, CursorShape, CursorStyle, EventBatch, Rgb, Size, StringTerm, Terminal,
    VtEvent,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The crate version as DA2 reports it: `major * 10000 + minor * 100 + patch`.
fn da2_version() -> u32 {
    let mut parts = VERSION
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0).min(99));
    let mut next = || parts.next().unwrap_or(0);
    let (major, minor, patch) = (next(), next(), next());
    major * 10_000 + minor * 100 + patch
}

fn da2_reply() -> Vec<u8> {
    format!("\x1b[>0;{};1c", da2_version()).into_bytes()
}

struct Harness {
    term: Terminal,
    batch: EventBatch,
}

impl Harness {
    fn new() -> Harness {
        Harness::sized(80, 24, Config::default())
    }

    fn with(config: Config) -> Harness {
        Harness::sized(80, 24, config)
    }

    fn sized(cols: u16, rows: u16, config: Config) -> Harness {
        Harness {
            term: Terminal::new(Size { rows, cols }, config),
            batch: EventBatch::new(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.term.feed(bytes, &mut self.batch, Instant::now());
    }

    /// The `Reply` events of the last feed, one entry each, in order.
    fn replies(&self) -> Vec<Vec<u8>> {
        self.batch
            .iter()
            .filter_map(|event| match event {
                VtEvent::Reply(span) => Some(self.batch.bytes(*span).to_vec()),
                _ => None,
            })
            .collect()
    }

    /// All reply bytes of `query`, concatenated.
    fn ask(&mut self, query: &[u8]) -> Vec<u8> {
        self.feed(query);
        self.replies().concat()
    }

    fn unhandled(&self) -> u32 {
        self.term.stats().unhandled_sequences
    }
}

fn readback() -> Harness {
    Harness::with(Config {
        allow_screen_readback: true,
        ..Config::default()
    })
}

fn named(name: &str) -> Harness {
    Harness::with(Config {
        product_name: Some(name.to_owned().into()),
        ..Config::default()
    })
}

#[test]
fn da1() {
    for query in [&b"\x1b[c"[..], b"\x1b[0c", b"\x1bZ"] {
        assert_eq!(Harness::new().ask(query), b"\x1b[?62;4;22c", "{query:?}");
    }
    let mut harness = Harness::new();
    assert!(harness.ask(b"\x1b[1c").is_empty());
    assert_eq!(harness.unhandled(), 1);
}

#[test]
fn da2() {
    for query in [&b"\x1b[>c"[..], b"\x1b[>0c"] {
        assert_eq!(Harness::new().ask(query), da2_reply(), "{query:?}");
    }
    let mut harness = Harness::new();
    assert!(harness.ask(b"\x1b[>1c").is_empty());
    assert_eq!(harness.unhandled(), 1);
}

#[test]
fn da2_follows_a_versioned_product_name() {
    let crate_version = da2_version();
    for (name, version) in [
        ("Acme(1.4.0)", 10_400),
        ("Acme(2)", 20_000),
        ("P(7.4294967296.9)", 70_009),
        ("P(1.200.3)", 19_903),
        ("P(-1.2.3)", crate_version),
        ("P()", crate_version),
        ("P(4294967296.0.0)", crate_version),
        ("P(x.1.2)", crate_version),
        ("no parentheses", crate_version),
        ("Acme 1.4.0)", crate_version),
    ] {
        let reply = named(name).ask(b"\x1b[>c");
        assert_eq!(
            reply,
            format!("\x1b[>0;{version};1c").into_bytes(),
            "{name}"
        );
    }
}

#[test]
fn da3() {
    for query in [&b"\x1b[=c"[..], b"\x1b[=0c"] {
        assert_eq!(
            Harness::new().ask(query),
            b"\x1bP!|00000000\x1b\\",
            "{query:?}"
        );
    }
}

#[test]
fn dsr_status() {
    assert_eq!(Harness::new().ask(b"\x1b[5n"), b"\x1b[0n");
    let mut harness = Harness::new();
    assert!(harness.ask(b"\x1b[7n").is_empty());
    assert_eq!(harness.unhandled(), 1);
}

#[test]
fn cpr() {
    let mut harness = Harness::new();
    assert_eq!(harness.ask(b"\x1b[6n"), b"\x1b[1;1R");
    assert_eq!(harness.ask(b"\x1b[5;12H\x1b[6n"), b"\x1b[5;12R");
    // With DECOM the row is relative to the region top.
    assert_eq!(
        harness.ask(b"\x1b[10;20r\x1b[?6h\x1b[3;4H\x1b[6n"),
        b"\x1b[3;4R"
    );
    // A pending wrap reports the last column.
    let mut narrow = Harness::sized(4, 3, Config::default());
    assert_eq!(narrow.ask(b"abcd\x1b[6n"), b"\x1b[1;4R");
}

#[test]
fn decxcpr() {
    let mut harness = Harness::new();
    assert_eq!(harness.ask(b"\x1b[?6n"), b"\x1b[?1;1;1R");
    assert_eq!(harness.ask(b"\x1b[5;12H\x1b[?6n"), b"\x1b[?5;12;1R");
    assert_eq!(
        harness.ask(b"\x1b[10;20r\x1b[?6h\x1b[3;4H\x1b[?6n"),
        b"\x1b[?3;4;1R"
    );
}

#[test]
fn decrqm_ansi() {
    let mut harness = Harness::new();
    assert_eq!(harness.ask(b"\x1b[4$p"), b"\x1b[4;2$y");
    harness.feed(b"\x1b[4h");
    assert_eq!(harness.ask(b"\x1b[4$p"), b"\x1b[4;1$y");
    assert_eq!(harness.ask(b"\x1b[20$p"), b"\x1b[20;2$y");
    assert_eq!(harness.ask(b"\x1b[99$p"), b"\x1b[99;0$y");
    assert_eq!(harness.ask(b"\x1b[$p"), b"\x1b[0;0$y");
}

#[test]
fn decrqm_private() {
    let mut harness = Harness::new();
    assert_eq!(harness.ask(b"\x1b[?7$p"), b"\x1b[?7;1$y");
    assert_eq!(harness.ask(b"\x1b[?1$p"), b"\x1b[?1;2$y");
    harness.feed(b"\x1b[?1h");
    assert_eq!(harness.ask(b"\x1b[?1$p"), b"\x1b[?1;1$y");
    assert_eq!(harness.ask(b"\x1b[?3$p"), b"\x1b[?3;0$y");
    assert_eq!(harness.ask(b"\x1b[?9001$p"), b"\x1b[?9001;2$y");
    assert_eq!(harness.ask(b"\x1b[?1048$p"), b"\x1b[?1048;2$y");
    assert_eq!(harness.ask(b"\x1b[?9999$p"), b"\x1b[?9999;0$y");
    harness.feed(b"\x1b[?1049h");
    assert_eq!(harness.ask(b"\x1b[?1049$p"), b"\x1b[?1049;1$y");
    assert_eq!(harness.ask(b"\x1b[?47$p"), b"\x1b[?47;1$y");
}

#[test]
fn xtversion() {
    let expected = format!("\x1bP>|oneterm-vt({VERSION})\x1b\\").into_bytes();
    for query in [&b"\x1b[>q"[..], b"\x1b[>0q"] {
        assert_eq!(Harness::new().ask(query), expected, "{query:?}");
    }
    let mut harness = Harness::new();
    assert!(harness.ask(b"\x1b[>1q").is_empty());
    assert_eq!(harness.unhandled(), 1);
}

#[test]
fn xtversion_cleans_the_product_name() {
    assert_eq!(
        named("Acme(1.4.0)").ask(b"\x1b[>q"),
        b"\x1bP>|Acme(1.4.0)\x1b\\"
    );
    // A control character cannot end the string early.
    assert_eq!(named("Ev\x1b\\il").ask(b"\x1b[>q"), b"\x1bP>|Ev\\il\x1b\\");
    assert_eq!(
        named("a\u{7f}b\u{85}c\u{0}d\u{1f}").ask(b"\x1b[>q"),
        b"\x1bP>|abcd\x1b\\"
    );
    // The cap is 64 bytes, cut at a character boundary; a character that does not fit ends
    // the name even when a shorter one follows.
    let cut = format!("{}{}", "x".repeat(70), "y");
    assert_eq!(
        named(&cut).ask(b"\x1b[>q"),
        format!("\x1bP>|{}\x1b\\", "x".repeat(64)).into_bytes()
    );
    let straddle = format!("{}\u{1F600}z", "x".repeat(62));
    assert_eq!(
        named(&straddle).ask(b"\x1b[>q"),
        format!("\x1bP>|{}\x1b\\", "x".repeat(62)).into_bytes()
    );
    let wide = format!("{}\u{1F600}", "x".repeat(60));
    assert_eq!(
        named(&wide).ask(b"\x1b[>q"),
        format!("\x1bP>|{wide}\x1b\\").into_bytes()
    );
    // Nothing left: the engine name.
    assert_eq!(
        named("\x1b\x07\u{9b}").ask(b"\x1b[>q"),
        format!("\x1bP>|oneterm-vt({VERSION})\x1b\\").into_bytes()
    );
    assert_eq!(named("").ask(b"\x1b[>q"), named("\x01").ask(b"\x1b[>q"));
}

#[test]
fn config_returns_the_cleaned_product_name() {
    let harness = named("Ev\x1b\\il");
    assert_eq!(
        harness.term.config().product_name.as_deref(),
        Some("Ev\\il")
    );
    assert_eq!(named("\x01").term.config().product_name, None);
}

#[test]
fn kitty_flags_query() {
    let mut harness = Harness::new();
    assert_eq!(harness.ask(b"\x1b[?u"), b"\x1b[?0u");
    assert_eq!(harness.ask(b"\x1b[>5u\x1b[?u"), b"\x1b[?5u");
    assert_eq!(harness.ask(b"\x1b[=8;2u\x1b[?u"), b"\x1b[?13u");
}

#[test]
fn modify_other_keys_query() {
    let mut harness = Harness::new();
    assert_eq!(harness.ask(b"\x1b[?4m"), b"\x1b[>4;0m");
    assert_eq!(harness.ask(b"\x1b[>4;2m\x1b[?4m"), b"\x1b[>4;2m");
    assert!(harness.ask(b"\x1b[?5m").is_empty());
    assert_eq!(harness.unhandled(), 1);
}

#[test]
fn text_area_in_pixels() {
    // 24 x 80 cells of the fallback 10 x 20 pixel cell until the embedder sets one.
    assert_eq!(Harness::new().ask(b"\x1b[14t"), b"\x1b[4;480;800t");
}

#[test]
fn text_area_in_cells() {
    assert_eq!(Harness::new().ask(b"\x1b[18t"), b"\x1b[8;24;80t");
    assert_eq!(
        Harness::sized(132, 50, Config::default()).ask(b"\x1b[18t"),
        b"\x1b[8;50;132t"
    );
}

#[test]
fn decrqcra_is_gated() {
    let mut harness = Harness::new();
    assert!(harness.ask(b"A\x1b[1;1;1;1;1;1*y").is_empty());
    assert_eq!(harness.unhandled(), 1);
}

#[test]
fn decrqcra_sums_the_rectangle() {
    let mut harness = readback();
    assert_eq!(harness.ask(b"\x1b[1;1;1;1;1;1*y"), b"\x1bP1!~0020\x1b\\");
    assert_eq!(
        harness.ask(b"A\x1b[7;1;1;1;1;1*y"),
        b"\x1bP7!~0041\x1b\\",
        "the id is echoed"
    );
    // Without a rectangle the sum covers the whole 80x24 screen: 1919 blanks and the A.
    assert_eq!(harness.ask(b"\x1b[9*y"), b"\x1bP9!~F021\x1b\\");
    // Attributes do not change the sum.
    assert_eq!(
        harness.ask(b"\x1b[1;4;31mB\x1b[0m\x1b[1;1;1;2;1;2*y"),
        b"\x1bP1!~0042\x1b\\"
    );
}

#[test]
fn decrqcra_counts_every_scalar_of_a_cluster() {
    let mut harness = readback();
    harness.feed("e\u{301}".as_bytes());
    assert_eq!(harness.ask(b"\x1b[1;1;1;1;1;1*y"), b"\x1bP1!~0366\x1b\\");
}

#[test]
fn decrqcra_with_origin_mode_is_clamped_to_the_region() {
    let mut harness = readback();
    harness.feed(b"\x1b[1;1Ha\x1b[2;1Hb\x1b[3;1Hx\x1b[4;1Hy\x1b[5;1Hz\x1b[6;1Hc");
    harness.feed(b"\x1b[3;5r\x1b[?6h");
    // Rows 1..99 inside the region are screen rows 3..5: x + y + z.
    assert_eq!(harness.ask(b"\x1b[1;1;1;1;99;1*y"), b"\x1bP1!~016B\x1b\\");
    // Without origin mode the same rectangle starts at the screen top.
    harness.feed(b"\x1b[?6l");
    assert_eq!(
        harness.ask(b"\x1b[1;1;1;1;3;1*y"),
        format!("\x1bP1!~{:04X}\x1b\\", 97 + 98 + 120).into_bytes()
    );
}

#[test]
fn decrqcra_never_reads_the_scrollback() {
    let mut harness = Harness::with(Config {
        allow_screen_readback: true,
        ..Config::default()
    });
    harness.term.resize(
        Size { rows: 3, cols: 80 },
        oneterm_vt::ResizePolicy::BottomAnchor,
    );
    harness.feed(b"A\r\nB\r\nC\r\nD");
    // A scrolled into history; B + C + D remain visible.
    assert_eq!(
        harness.ask(b"\x1b[1;1;1;1;99;1*y"),
        format!("\x1bP1!~{:04X}\x1b\\", 66 + 67 + 68).into_bytes()
    );
}

#[test]
fn decrqcra_with_a_reversed_rectangle_sums_nothing() {
    let mut harness = readback();
    harness.feed(b"abc");
    assert_eq!(harness.ask(b"\x1b[1;1;3;3;1;1*y"), b"\x1bP1!~0000\x1b\\");
}

#[test]
fn decrqss_known_requests() {
    let mut harness = Harness::new();
    assert_eq!(harness.ask(b"\x1bP$qm\x1b\\"), b"\x1bP1$r0m\x1b\\");
    assert_eq!(harness.ask(b"\x1bP$qr\x1b\\"), b"\x1bP1$r1;24r\x1b\\");
    assert_eq!(
        harness.ask(b"\x1b[5;10r\x1bP$qr\x1b\\"),
        b"\x1bP1$r5;10r\x1b\\"
    );
    assert_eq!(harness.ask(b"\x1bP$q q\x1b\\"), b"\x1bP1$r2 q\x1b\\");
    assert_eq!(
        harness.ask(b"\x1b[5 q\x1bP$q q\x1b\\"),
        b"\x1bP1$r5 q\x1b\\"
    );
    assert_eq!(
        harness.ask(b"\x1b[4 q\x1bP$q q\x1b\\"),
        b"\x1bP1$r4 q\x1b\\"
    );
    assert_eq!(
        harness.ask(b"\x1b[2 q\x1bP$q q\x1b\\"),
        b"\x1bP1$r2 q\x1b\\"
    );
    assert_eq!(harness.ask(b"\x1bP$q\"q\x1b\\"), b"\x1bP1$r0\"q\x1b\\");
    assert_eq!(harness.ask(b"\x1bP$q\"p\x1b\\"), b"\x1bP1$r62;1\"p\x1b\\");
}

#[test]
fn decrqss_sgr_reports_the_pen() {
    let mut harness = Harness::new();
    harness.feed(b"\x1b[1;3;4:3;9;31;48;5;200;58;2;1;2;3m");
    assert_eq!(
        harness.ask(b"\x1bP$qm\x1b\\"),
        b"\x1bP1$r0;1;3;9;4:3;31;48;5;200;58;2;1;2;3m\x1b\\"
    );
    harness.feed(b"\x1b[0;53;97;104;38;2;1;2;3m");
    assert_eq!(
        harness.ask(b"\x1bP$qm\x1b\\"),
        b"\x1bP1$r0;53;38;2;1;2;3;104m\x1b\\"
    );
}

#[test]
fn decrqss_refusals() {
    let mut harness = Harness::new();
    for request in [
        &b""[..],
        b"s",
        b"$}",
        b"*x",
        b"$|",
        b"*|",
        b"mm",
        "é".as_bytes(),
        b"\"",
        b"M",
    ] {
        let mut query = b"\x1bP$q".to_vec();
        query.extend_from_slice(request);
        query.extend_from_slice(b"\x1b\\");
        assert_eq!(harness.ask(&query), b"\x1bP0$r\x1b\\", "{request:?}");
    }
}

#[test]
fn decrqss_reports_the_stored_cursor_style_not_the_effective_one() {
    let config = Config {
        default_cursor_style: CursorStyle {
            shape: CursorShape::HollowBlock,
            blinking: false,
        },
        ..Config::default()
    };
    let mut steady = Harness::with(config);
    assert_eq!(steady.ask(b"\x1bP$q q\x1b\\"), b"\x1bP1$r2 q\x1b\\");
    let config = Config {
        default_cursor_style: CursorStyle {
            shape: CursorShape::HollowBlock,
            blinking: true,
        },
        ..Config::default()
    };
    let mut blinking = Harness::with(config);
    assert_eq!(blinking.ask(b"\x1bP$q q\x1b\\"), b"\x1bP1$r1 q\x1b\\");
    // Hiding the cursor changes the effective style, not the stored one.
    assert_eq!(
        blinking.ask(b"\x1b[?25l\x1bP$q q\x1b\\"),
        b"\x1bP1$r1 q\x1b\\"
    );
}

#[test]
fn xtgettcap_known_names() {
    let mut harness = Harness::new();
    assert_eq!(
        harness.ask(b"\x1bP+q544e\x1b\\"),
        b"\x1bP1+r544e=787465726D2D323536636F6C6F72\x1b\\"
    );
    assert_eq!(
        harness.ask(b"\x1bP+q436f\x1b\\"),
        b"\x1bP1+r436f=323536\x1b\\"
    );
    assert_eq!(
        harness.ask(b"\x1bP+q436F\x1b\\"),
        b"\x1bP1+r436F=323536\x1b\\"
    );
    assert_eq!(
        harness.ask(b"\x1bP+q524742\x1b\\"),
        b"\x1bP1+r524742=\x1b\\"
    );
    assert_eq!(
        harness.ask(b"\x1bP+q62656C\x1b\\"),
        b"\x1bP1+r62656C=07\x1b\\",
        "bel"
    );
    assert_eq!(
        harness.ask(b"\x1bP+q686F6D65\x1b\\"),
        b"\x1bP1+r686F6D65=1B5B48\x1b\\",
        "home"
    );
}

#[test]
fn xtgettcap_terminal_name_follows_the_product() {
    assert_eq!(
        named("Acme Term (1.0)").ask(b"\x1bP+q544e\x1b\\"),
        b"\x1bP1+r544e=41636D65205465726D\x1b\\"
    );
    assert_eq!(
        named("(1.0)").ask(b"\x1bP+q544e\x1b\\"),
        b"\x1bP1+r544e=787465726D2D323536636F6C6F72\x1b\\"
    );
}

#[test]
fn xtgettcap_answers_each_name_in_order() {
    let mut harness = Harness::new();
    harness.feed(b"\x1bP+q544e;436f;524742\x1b\\");
    assert_eq!(
        harness.replies(),
        [
            b"\x1bP1+r544e=787465726D2D323536636F6C6F72\x1b\\".to_vec(),
            b"\x1bP1+r436f=323536\x1b\\".to_vec(),
            b"\x1bP1+r524742=\x1b\\".to_vec(),
        ]
    );
}

#[test]
fn xtgettcap_unknown_and_malformed_names() {
    let mut harness = Harness::new();
    assert_eq!(harness.ask(b"\x1bP+q7a7a\x1b\\"), b"\x1bP0+r7a7a\x1b\\");
    assert_eq!(harness.ask(b"\x1bP+q544\x1b\\"), b"\x1bP0+r544\x1b\\");
    assert_eq!(harness.ask(b"\x1bP+qzz\x1b\\"), b"\x1bP0+r\x1b\\");
    assert_eq!(harness.ask(b"\x1bP+q\x1b\\"), b"\x1bP0+r\x1b\\");
    assert_eq!(harness.unhandled(), 0);
}

#[test]
fn osc_colour_queries_are_answered_with_color_reply() {
    let mut harness = Harness::new();
    harness.feed(b"\x1b]11;?\x07\x1b]4;1;?\x1b\\");
    let queries: Vec<(ColorKey, StringTerm)> = harness
        .batch
        .iter()
        .filter_map(|event| match event {
            VtEvent::ColorQuery { key, terminator } => Some((*key, *terminator)),
            _ => None,
        })
        .collect();
    assert_eq!(
        queries,
        [
            (ColorKey::Background, StringTerm::Bel),
            (ColorKey::Palette(1), StringTerm::St)
        ]
    );
    assert_eq!(
        ColorKey::Background.color_reply(
            Rgb {
                r: 30,
                g: 30,
                b: 30
            },
            StringTerm::Bel
        ),
        b"\x1b]11;rgb:1e1e/1e1e/1e1e\x07"
    );
    assert_eq!(
        ColorKey::Palette(1).color_reply(Rgb { r: 205, g: 0, b: 0 }, StringTerm::St),
        b"\x1b]4;1;rgb:cdcd/0000/0000\x1b\\"
    );
}

#[test]
fn replies_keep_stream_order() {
    let mut harness = Harness::new();
    harness.feed(b"\x1b[5n\x1b[c\x1b[6n");
    assert_eq!(
        harness.replies(),
        [
            b"\x1b[0n".to_vec(),
            b"\x1b[?62;4;22c".to_vec(),
            b"\x1b[1;1R".to_vec()
        ]
    );
    harness.feed(b"ab\x1b[6n");
    assert_eq!(harness.replies(), [b"\x1b[1;3R".to_vec()]);
}
