//! The deterministic benchmark streams: ten generators, no RNG, no files.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

pub(crate) const COLUMNS: usize = 160;
pub(crate) const LINES: usize = 45;

pub(crate) struct Fixture {
    pub(crate) name: &'static str,
    pub(crate) about: &'static str,
    /// Prefix, then whole units until at least `target` bytes (always at least one unit).
    pub(crate) make: fn(target: usize) -> Vec<u8>,
}

const SIXEL_UNIT: &[u8] =
    b"\x1bPq\"1;1;12;12#0;2;100;0;0#1;2;0;100;0#0~~~~~~~~~~~~-#1~~~~~~~~~~~~\x1b\\\r\n";
const OSC_UNIT: &[u8] = b"\x1b]9;7;state=running;title=build\x07building the workspace\r\n";

pub(crate) const FIXTURES: [Fixture; 10] = [
    Fixture {
        name: "plain_ascii",
        about: "plain ASCII log lines",
        make: plain_ascii,
    },
    Fixture {
        name: "long_lines",
        about: "lines 5x the width, each wraps",
        make: long_lines,
    },
    Fixture {
        name: "heavy_sgr",
        about: "a 24-bit SGR per char",
        make: heavy_sgr,
    },
    Fixture {
        name: "tui_redraw",
        about: "CUP + short text",
        make: tui_redraw,
    },
    Fixture {
        name: "scroll_region",
        about: "DECSTBM, then line feeds",
        make: scroll_region,
    },
    Fixture {
        name: "cjk_wide",
        about: "wide CJK fills every line",
        make: cjk_wide,
    },
    Fixture {
        name: "dense_cells",
        about: "256-colour SGR per cell",
        make: dense_cells,
    },
    Fixture {
        name: "scrolling",
        about: "grid-width plain lines",
        make: scrolling,
    },
    Fixture {
        name: "sixel",
        about: "small Sixel images",
        make: sixel,
    },
    Fixture {
        name: "osc_9_7",
        about: "an OSC 9;7 status before every line",
        make: osc_9_7,
    },
];

/// The fixtures a measure command runs: all of them in table order, or the one `name` names.
pub(crate) fn select(name: Option<&str>) -> Result<Vec<&'static Fixture>> {
    let all: &'static [Fixture] = &FIXTURES;
    let Some(name) = name else {
        return Ok(all.iter().collect());
    };
    all.iter()
        .find(|fixture| fixture.name == name)
        .map(|fixture| vec![fixture])
        .with_context(|| {
            let names: Vec<&str> = all.iter().map(|fixture| fixture.name).collect();
            format!("unknown fixture '{name}'; one of: {}", names.join(", "))
        })
}

/// `prefix`, then `unit(i, out)` for `i = 0, 1, ...` until the stream holds `target` bytes.
fn build(target: usize, prefix: &[u8], mut unit: impl FnMut(usize, &mut Vec<u8>)) -> Vec<u8> {
    let mut out = Vec::with_capacity(target + 4096);
    out.extend_from_slice(prefix);
    let mut index = 0;
    loop {
        unit(index, &mut out);
        index += 1;
        if out.len() >= target {
            return out;
        }
    }
}

/// Appends formatted text; writing to a `Vec<u8>` cannot fail.
fn put(out: &mut Vec<u8>, args: std::fmt::Arguments<'_>) {
    let mut text = String::new();
    if text.write_fmt(args).is_ok() {
        out.extend_from_slice(text.as_bytes());
    }
}

fn letter(index: usize) -> u8 {
    b'a' + (index % 26) as u8
}

fn plain_ascii(target: usize) -> Vec<u8> {
    let line = b"the quick brown fox jumps over the lazy dog 0123456789 -- log line\r\n";
    build(target, b"", |_, out| out.extend_from_slice(line))
}

fn long_lines(target: usize) -> Vec<u8> {
    build(target, b"", |_, out| {
        out.extend((0..5 * COLUMNS).map(letter));
        out.extend_from_slice(b"\r\n");
    })
}

fn heavy_sgr(target: usize) -> Vec<u8> {
    build(target, b"", |i, out| {
        put(
            out,
            format_args!("\x1b[38;2;{};{};{}m", i % 256, (i / 3) % 256, (i / 7) % 256),
        );
        out.push(letter(i));
        if (i + 1) % COLUMNS == 0 {
            out.extend_from_slice(b"\r\n");
        }
    })
}

fn tui_redraw(target: usize) -> Vec<u8> {
    build(target, b"", |i, out| {
        put(
            out,
            format_args!("\x1b[{};{}Hstat: 42", i % LINES + 1, (i * 7) % COLUMNS + 1),
        );
    })
}

fn scroll_region(target: usize) -> Vec<u8> {
    let prefix = format!("\x1b[2;{}r", LINES - 1);
    build(target, prefix.as_bytes(), |_, out| {
        out.extend_from_slice(b"tail output line ");
        out.extend_from_slice(&[b'-'; 30]);
        out.extend_from_slice(b"\r\n");
    })
}

fn cjk_wide(target: usize) -> Vec<u8> {
    build(target, b"", |_, out| {
        for _ in 0..COLUMNS / 2 {
            out.extend_from_slice("\u{56FD}".as_bytes());
        }
        out.extend_from_slice(b"\r\n");
    })
}

fn dense_cells(target: usize) -> Vec<u8> {
    build(target, b"", |_, out| {
        out.extend_from_slice(b"\x1b[H");
        for row in 0..LINES {
            for col in 0..COLUMNS {
                put(
                    out,
                    format_args!("\x1b[48;5;{}m", (row * COLUMNS + col) % 256),
                );
                out.push(letter(row + col));
            }
        }
    })
}

fn scrolling(target: usize) -> Vec<u8> {
    build(target, b"", |_, out| {
        out.extend_from_slice(&[b'.'; COLUMNS]);
        out.extend_from_slice(b"\r\n");
    })
}

fn sixel(target: usize) -> Vec<u8> {
    build(target, b"", |_, out| out.extend_from_slice(SIXEL_UNIT))
}

fn osc_9_7(target: usize) -> Vec<u8> {
    build(target, b"", |_, out| out.extend_from_slice(OSC_UNIT))
}

/// Writes `DIR/<name>.vt` for every fixture (`mib` MiB each) and returns one report line per file.
pub(crate) fn write_fixtures(dir: &Path, mib: usize) -> Result<Vec<String>> {
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let mut lines = Vec::with_capacity(FIXTURES.len());
    for fixture in &FIXTURES {
        let bytes = (fixture.make)(mib << 20);
        let path = dir.join(format!("{}.vt", fixture.name));
        fs::write(&path, &bytes).with_context(|| format!("cannot write {}", path.display()))?;
        lines.push(format!(
            "wrote {} ({} bytes): {}",
            path.display(),
            bytes.len(),
            fixture.about
        ));
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use oneterm_vt::parser::{Dispatch, Intermediates, OscParams, Params, Parser, StringTerm};

    use super::*;

    fn make(name: &str, target: usize) -> Vec<u8> {
        let fixture = FIXTURES.iter().find(|f| f.name == name).unwrap();
        (fixture.make)(target)
    }

    #[test]
    fn select_by_name() {
        let one = select(Some("heavy_sgr")).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].name, "heavy_sgr");
        let all = select(None).unwrap();
        let names: Vec<&str> = all.iter().map(|fixture| fixture.name).collect();
        let table: Vec<&str> = FIXTURES.iter().map(|fixture| fixture.name).collect();
        assert_eq!(names, table);
        let error = select(Some("nope")).err().unwrap();
        assert_eq!(
            format!("{error:#}"),
            format!("unknown fixture 'nope'; one of: {}", table.join(", "))
        );
    }

    #[test]
    fn names_are_unique_and_in_table_order() {
        let names: Vec<&str> = FIXTURES.iter().map(|f| f.name).collect();
        assert_eq!(
            names,
            [
                "plain_ascii",
                "long_lines",
                "heavy_sgr",
                "tui_redraw",
                "scroll_region",
                "cjk_wide",
                "dense_cells",
                "scrolling",
                "sixel",
                "osc_9_7"
            ]
        );
    }

    #[test]
    fn first_unit_matches_the_table() {
        let long: Vec<u8> = (0..800)
            .map(|k| b'a' + (k % 26) as u8)
            .chain(*b"\r\n")
            .collect();
        let cjk = "\u{56FD}".repeat(80) + "\r\n";
        let golden: [(&str, Vec<u8>); 9] = [
            (
                "plain_ascii",
                b"the quick brown fox jumps over the lazy dog 0123456789 -- log line\r\n".to_vec(),
            ),
            ("long_lines", long),
            ("heavy_sgr", b"\x1b[38;2;0;0;0ma".to_vec()),
            ("tui_redraw", b"\x1b[1;1Hstat: 42".to_vec()),
            (
                "scroll_region",
                [
                    b"\x1b[2;44r".as_slice(),
                    b"tail output line ",
                    &[b'-'; 30],
                    b"\r\n",
                ]
                .concat(),
            ),
            ("cjk_wide", cjk.into_bytes()),
            ("scrolling", [[b'.'; 160].as_slice(), b"\r\n"].concat()),
            ("sixel", SIXEL_UNIT.to_vec()),
            ("osc_9_7", OSC_UNIT.to_vec()),
        ];
        for (name, expected) in golden {
            assert_eq!(make(name, 1), expected, "{name}");
        }
        assert_eq!(SIXEL_UNIT.len(), 70);
        assert_eq!(OSC_UNIT.len(), 56);
    }

    #[test]
    fn dense_cells_first_frame() {
        let frame = make("dense_cells", 1);
        assert_eq!(
            &frame[..32],
            b"\x1b[H\x1b[48;5;0ma\x1b[48;5;1mb\x1b[48;5;2m"
        );
        // 7200 cells: SGR with 1, 2 or 3 digits plus 8 bytes of framing, one letter each.
        let sgr_bytes: usize = (0..LINES * COLUMNS)
            .map(|cell| 8 + (cell % 256).to_string().len())
            .sum();
        assert_eq!(frame.len(), 3 + sgr_bytes + LINES * COLUMNS);
    }

    #[test]
    fn heavy_sgr_breaks_lines_every_160_units() {
        let stream = make("heavy_sgr", 1 << 16);
        let mut units = Vec::new();
        let mut start = 0;
        for index in 1..stream.len() {
            if stream[index] == 0x1B {
                units.push(&stream[start..index]);
                start = index;
            }
        }
        assert!(units[159].ends_with(b"\r\n"));
        assert!(!units[160].ends_with(b"\r\n"));
    }

    #[test]
    fn tui_redraw_second_unit() {
        let stream = make("tui_redraw", 40);
        let second = b"\x1b[2;8Hstat: 42";
        assert!(stream.windows(second.len()).any(|w| w == second));
    }

    #[test]
    fn generators_are_deterministic_and_stop_at_the_target() {
        for fixture in &FIXTURES {
            let first = (fixture.make)(100_000);
            assert_eq!(first, (fixture.make)(100_000), "{}", fixture.name);
            // Unit sizes vary (an SGR unit grows with its numbers); 32 covers the SGR and CUP units.
            let longest_unit = (fixture.make)(1).len().max(32);
            assert!(first.len() >= 100_000, "{}", fixture.name);
            assert!(first.len() < 100_000 + longest_unit, "{}", fixture.name);
        }
    }

    #[derive(Default)]
    struct Tally {
        printed: usize,
        linefeeds: usize,
        csis: usize,
        oscs: usize,
        hooks: usize,
        unhooks: usize,
        flagged: usize,
    }

    impl Dispatch for Tally {
        fn print_str(&mut self, text: &str) {
            self.printed += text.chars().count();
        }
        fn execute(&mut self, byte: u8) {
            self.linefeeds += usize::from(byte == b'\n');
        }
        fn csi(&mut self, _: &Params, _: &Intermediates, ignore: bool, _: u8) {
            self.csis += 1;
            self.flagged += usize::from(ignore);
        }
        fn osc(&mut self, _: Option<u32>, _: &OscParams<'_>, _: StringTerm, truncated: bool) {
            self.oscs += 1;
            self.flagged += usize::from(truncated);
        }
        fn dcs_hook(&mut self, _: &Params, _: &Intermediates, ignore: bool, _: u8) {
            self.hooks += 1;
            self.flagged += usize::from(ignore);
        }
        fn dcs_unhook(&mut self, aborted: bool) {
            self.unhooks += 1;
            self.flagged += usize::from(aborted);
        }
    }

    fn tally(name: &str) -> Tally {
        let mut tally = Tally::default();
        Parser::new().advance(&mut tally, &make(name, 1 << 16));
        assert_eq!(
            tally.flagged, 0,
            "{name} produced ignored/truncated/aborted items"
        );
        tally
    }

    #[test]
    fn every_fixture_parses_cleanly() {
        for fixture in &FIXTURES {
            tally(fixture.name);
        }
        let sgr = tally("heavy_sgr");
        assert_eq!(sgr.csis, sgr.printed);
        let osc = tally("osc_9_7");
        assert_eq!(osc.oscs, osc.linefeeds);
        let sixel = tally("sixel");
        assert_eq!(
            (sixel.hooks, sixel.unhooks),
            (sixel.linefeeds, sixel.linefeeds)
        );
        let dense = tally("dense_cells");
        let frames = 1;
        assert_eq!(dense.csis, dense.printed + frames);
    }

    #[test]
    fn write_fixtures_creates_one_file_each() {
        struct RemoveOnDrop(std::path::PathBuf);
        impl Drop for RemoveOnDrop {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let dir = std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.tmp/fixtures-test"
        ))
        .join("write_fixtures_creates_one_file_each");
        let _guard = RemoveOnDrop(dir.clone());
        let lines = write_fixtures(&dir, 1).unwrap();
        assert_eq!(lines.len(), 10);
        for fixture in &FIXTURES {
            let bytes = fs::read(dir.join(format!("{}.vt", fixture.name))).unwrap();
            assert!(bytes.len() >= 1 << 20);
        }
    }
}
