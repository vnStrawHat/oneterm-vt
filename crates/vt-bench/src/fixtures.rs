//! The deterministic benchmark streams: fourteen release generators and nine compare-only ones, no RNG state.

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

pub(crate) const FIXTURES: [Fixture; 14] = [
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
    Fixture {
        name: "grapheme_clusters",
        about: "emoji, flags and combining marks under ?2027",
        make: grapheme_clusters,
    },
    Fixture {
        name: "hyperlinks",
        about: "OSC 8 links with 200-1000 byte URIs",
        make: hyperlinks,
    },
    Fixture {
        name: "mode_churn",
        about: "alt screen and mode toggles, a TUI opening and closing",
        make: mode_churn,
    },
    Fixture {
        name: "sixel_large",
        about: "640x480 to 1024x768 Sixel images",
        make: sixel_large,
    },
];

/// The nine streams only the cross-engine comparison runs (`compare/`, spec 0016). They stay
/// out of `FIXTURES` so release tiers, the release bench, `grid --check` and the history
/// charts never see them; `fixtures --compare` writes them and `--fixture NAME` finds them.
pub(crate) const COMPARE_FIXTURES: [Fixture; 9] = [
    Fixture {
        name: "cursor_motion",
        about: "a spiral of CUP, one letter per ring cell",
        make: cursor_motion,
    },
    Fixture {
        name: "light_cells",
        about: "alt screen, home, then a full screen of one letter",
        make: light_cells,
    },
    Fixture {
        name: "short_lines",
        about: "one-letter lines scrolling the whole screen",
        make: short_lines,
    },
    Fixture {
        name: "scroll_region_small",
        about: "one-letter lines scrolling a 22-line region",
        make: scroll_region_small,
    },
    Fixture {
        name: "csi_mix",
        about: "a weighted mix of short CSI sequences",
        make: csi_mix,
    },
    Fixture {
        name: "combining_unique",
        about: "a base letter with three unique combining marks per cell",
        make: combining_unique,
    },
    Fixture {
        name: "long_osc",
        about: "8000-byte OSC strings between short lines",
        make: long_osc,
    },
    Fixture {
        name: "unicode_symbols",
        about: "vendored vtebench symbols stream",
        make: unicode_symbols,
    },
    Fixture {
        name: "vim_session",
        about: "vendored vtebench zsh and vim recording",
        make: vim_session,
    },
];

/// The fixtures a measure command runs: the release ones in table order, or the one `name`
/// names (release or compare-only).
pub(crate) fn select(name: Option<&str>) -> Result<Vec<&'static Fixture>> {
    let all: &'static [Fixture] = &FIXTURES;
    let Some(name) = name else {
        return Ok(all.iter().collect());
    };
    all.iter()
        .chain(&COMPARE_FIXTURES)
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

/// Text pieces for `grapheme_clusters` and the columns each takes under `?2027`.
const CLUSTER_ATOMS: [(&str, usize); 27] = [
    ("cluster ", 8),
    ("text ", 5),
    ("plain ", 6),
    ("quick ", 6),
    // ZWJ sequences: family, health worker (with VS16), technologist, astronaut.
    (
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}",
        2,
    ),
    ("\u{1F469}\u{200D}\u{2695}\u{FE0F}", 2),
    ("\u{1F468}\u{200D}\u{1F4BB}", 2),
    ("\u{1F9D1}\u{200D}\u{1F680}", 2),
    // VS16 and skin-tone modifiers.
    ("\u{2764}\u{FE0F}", 2),
    ("\u{2601}\u{FE0F}", 2),
    ("\u{1F44D}\u{1F3FD}", 2),
    ("\u{1F44B}\u{1F3FB}", 2),
    ("\u{1F9D1}\u{1F3FF}", 2),
    // Flags: regional indicator pairs.
    ("\u{1F1FB}\u{1F1F3}", 2),
    ("\u{1F1EF}\u{1F1F5}", 2),
    ("\u{1F1FA}\u{1F1F8}", 2),
    ("\u{1F1E9}\u{1F1EA}", 2),
    // Combining marks, the Vietnamese ones stacked.
    ("e\u{301}", 1),
    ("a\u{300}", 1),
    ("n\u{303}", 1),
    ("e\u{323}\u{302}", 1),
    ("a\u{306}\u{301}", 1),
    ("u\u{31B}\u{323}", 1),
    ("e\u{302}\u{300}", 1),
    ("o\u{31B}\u{309}", 1),
    ("y\u{301}", 1),
    ("a\u{323}\u{306}", 1),
];

/// A stateless 64-bit hash (the splitmix64 finalizer): the same seed always gives the same
/// number, so a generator needs no RNG state carried from unit to unit.
fn scramble(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A number in `0..bound` derived from `seed`.
fn pick(seed: u64, bound: usize) -> usize {
    (scramble(seed) % bound as u64) as usize
}

/// `?2027` on, then lines of mixed clusters and ASCII, each just short of the grid width.
fn grapheme_clusters(target: usize) -> Vec<u8> {
    build(target, b"\x1b[?2027h", |i, out| {
        let mut column = 0;
        for n in 0u64.. {
            let (text, width) = CLUSTER_ATOMS[pick((i as u64) << 16 | n, CLUSTER_ATOMS.len())];
            if column + width > COLUMNS - 2 {
                break;
            }
            out.extend_from_slice(text.as_bytes());
            column += width;
        }
        out.extend_from_slice(b"\r\n");
    })
}

/// Distinct URIs the `hyperlinks` stream cycles through: far below the table's 65 536 links
/// and 4 MiB, so nothing is ever refused.
const LINK_POOL: usize = 1024;
const LINK_HOSTS: [&str; 8] = [
    "docs.example.com",
    "git.example.org",
    "wiki.example.net",
    "cdn.example.com",
    "mail.example.org",
    "status.example.net",
    "api.example.com",
    "files.example.org",
];
const LINK_SEGMENTS: [&str; 16] = [
    "docs",
    "guide",
    "api",
    "reference",
    "releases",
    "issues",
    "blob",
    "main",
    "src",
    "terminal",
    "render",
    "v1.2.3",
    "changelog",
    "index.html",
    "search?q=grid&page=2",
    "download%20area",
];

/// The URI of link `k`: 200 to 1000 ASCII bytes, no `;`, so the whole OSC stays inline.
fn link_uri(k: usize) -> String {
    let length = 200 + pick(k as u64, 801);
    let mut uri = format!("https://{}/", LINK_HOSTS[k % LINK_HOSTS.len()]);
    for n in 0u64.. {
        if uri.len() >= length {
            break;
        }
        uri.push_str(LINK_SEGMENTS[pick((k as u64) << 16 | n, LINK_SEGMENTS.len())]);
        uri.push('/');
    }
    uri.truncate(length);
    uri
}

/// Two linked runs per line. Every third link is one of eight hot URIs, the rest walk the
/// pool; every fifth pool entry has no `id`. Open and close use `ST` or `BEL` by turns.
fn hyperlinks(target: usize) -> Vec<u8> {
    build(target, b"", |i, out| {
        out.extend_from_slice(b"see ");
        for slot in 0..2 {
            let seed = (i * 2 + slot) as u64;
            let k = if pick(seed, 3) == 0 {
                pick(seed ^ 0xA5A5, 8)
            } else {
                (i * 2 + slot) % LINK_POOL
            };
            let end: &[u8] = if (i + slot) % 2 == 0 {
                b"\x07"
            } else {
                b"\x1b\\"
            };
            out.extend_from_slice(b"\x1b]8;");
            if k % 5 != 0 {
                put(out, format_args!("id=l{k}"));
            }
            put(out, format_args!(";{}", link_uri(k)));
            out.extend_from_slice(end);
            put(out, format_args!("link {k} text"));
            out.extend_from_slice(b"\x1b]8;;");
            out.extend_from_slice(end);
            out.extend_from_slice(b" and ");
        }
        out.extend_from_slice(b"done\r\n");
    })
}

/// One TUI session per unit: enter the alternate screen and switch modes on, draw two
/// synchronized frames, then undo everything and print a shell line.
fn mode_churn(target: usize) -> Vec<u8> {
    build(target, b"", |i, out| {
        out.extend_from_slice(b"\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[?2004h\x1b[?1000h\x1b[?1006h");
        for frame in 0..2 {
            put(
                out,
                format_args!("\x1b[?2026h\x1b]0;editor {}\x07\x1b7", (i + frame) % 100),
            );
            put(out, format_args!("\x1b[2;{}r\x1b[0m", LINES - 1));
            put(out, format_args!("\x1b[1;{}Hstatus {i}", (i * 3) % 100 + 1));
            out.extend_from_slice(b"\x1b[1;32m ok \x1b[0m");
            put(out, format_args!("\x1b[{LINES};1H-- INSERT --\x1b[m\x1b[r"));
            out.extend_from_slice(b"\x1b8\x1b[?2026l");
        }
        out.extend_from_slice(b"\x1b]2;shell\x1b\\");
        out.extend_from_slice(b"\x1b[?1006l\x1b[?1000l\x1b[?2004l\x1b[?7h\x1b[?25h\x1b[?1049l");
        put(out, format_args!("$ exit {}\r\n", i % 256));
    })
}

/// Image sizes `sixel_large` cycles through, the largest first (so the first unit is the
/// longest) and all well under the 4096 px side limit.
const SIXEL_SIZES: [(u32, u32); 3] = [(1024, 768), (800, 600), (640, 480)];
const SIXEL_REGISTERS: u64 = 16;
const SIXEL_PASSES: u64 = 4;

/// One image per unit: a 16-colour palette, then per band four colour passes of varied runs,
/// a quarter of them skips that paint nothing.
fn sixel_large(target: usize) -> Vec<u8> {
    build(target, b"", |i, out| {
        let (width, height) = SIXEL_SIZES[i % SIXEL_SIZES.len()];
        let image = (i as u64) << 48;
        put(out, format_args!("\x1bPq\"1;1;{width};{height}"));
        for register in 0..SIXEL_REGISTERS {
            let h = scramble(image | register);
            put(
                out,
                format_args!(
                    "#{register};2;{};{};{}",
                    h % 101,
                    (h >> 8) % 101,
                    (h >> 16) % 101
                ),
            );
        }
        let bands = u64::from(height / 6);
        for band in 0..bands {
            for pass in 0..SIXEL_PASSES {
                let pass_seed = image | band << 32 | pass << 24;
                if pass > 0 {
                    out.push(b'$');
                }
                let register = scramble(pass_seed) % SIXEL_REGISTERS;
                put(out, format_args!("#{register}"));
                let (mut x, mut run) = (0, 0);
                while x < width {
                    let h = scramble(pass_seed | run);
                    let length = (1 + (h % 48) as u32).min(width - x);
                    let sixel = if (h >> 16).is_multiple_of(4) {
                        b'?'
                    } else {
                        0x40 + (h >> 24) as u8 % 63
                    };
                    if length == 1 {
                        out.push(sixel);
                    } else {
                        put(out, format_args!("!{length}{}", char::from(sixel)));
                    }
                    x += length;
                    run += 1;
                }
            }
            if band + 1 < bands {
                out.push(b'-');
            }
        }
        out.extend_from_slice(b"\x1b\\\r\n");
    })
}

/// The `(row, col)` cells (0-based) of the full-screen spiral, ring by ring: each ring runs
/// along its top edge, down its right edge, back along the bottom and up the left edge, so
/// every cell is visited once. A ring of one row or one column is a single pass.
fn spiral_cells(columns: usize, lines: usize) -> Vec<(usize, usize)> {
    let mut cells = Vec::with_capacity(columns * lines);
    for ring in 0.. {
        if 2 * ring >= lines.min(columns) {
            break;
        }
        let (top, bottom) = (ring, lines - 1 - ring);
        let (left, right) = (ring, columns - 1 - ring);
        cells.extend((left..=right).map(|col| (top, col)));
        if bottom > top {
            cells.extend((top + 1..=bottom).map(|row| (row, right)));
        }
        if bottom > top && right > left {
            cells.extend((left..right).rev().map(|col| (bottom, col)));
            cells.extend((top + 1..bottom).rev().map(|row| (row, left)));
        }
    }
    cells
}

/// One spiral over the grid per unit, one `CUP` and one letter per cell; the letter changes
/// from unit to unit.
fn cursor_motion(target: usize) -> Vec<u8> {
    let cells = spiral_cells(COLUMNS, LINES);
    build(target, b"", |i, out| {
        let mark = b'A' + (i % 26) as u8;
        for &(row, col) in &cells {
            put(out, format_args!("\x1b[{};{}H", row + 1, col + 1));
            out.push(mark);
        }
    })
}

/// The alternate screen, then per unit home and one letter in every cell, never scrolling.
fn light_cells(target: usize) -> Vec<u8> {
    build(target, b"\x1b[?1049h", |i, out| {
        out.extend_from_slice(b"\x1b[H");
        out.extend(std::iter::repeat_n(letter(i), COLUMNS * LINES));
    })
}

fn short_lines(target: usize) -> Vec<u8> {
    build(target, b"", |_, out| out.extend_from_slice(b"y\r\n"))
}

/// The alternate screen with a 22-line scroll region at the top; the rows below stay blank.
fn scroll_region_small(target: usize) -> Vec<u8> {
    build(target, b"\x1b[?1049h\x1b[1;22r", |_, out| {
        out.extend_from_slice(b"y\r\n");
    })
}

/// Short sequences of the kinds a TUI sends most (SGR with sub-parameters, cursor moves,
/// erases, REP), in rough proportion; `pick` chooses the kind, `detail` its parameters.
fn csi_mix(target: usize) -> Vec<u8> {
    build(target, b"", csi_mix_unit)
}

fn csi_mix_unit(i: usize, out: &mut Vec<u8>) {
    let detail = scramble((i as u64) ^ 0x5EED_0000_0000);
    match pick(i as u64, 100) {
        0..10 => {
            let length = 1 + (detail % 72) as usize;
            out.extend((0..length).map(|k| letter(i + k)));
            out.extend_from_slice(b"\r\n");
        }
        10..30 => out.extend_from_slice(b"\x1b[m\x1b[?1h\x1b[H"),
        30..40 => out.extend_from_slice(b"\x1b[1;2;3;4:3;31m"),
        40..50 => put(
            out,
            format_args!(
                "\x1b[38:5:{}m\x1b[48:2::{}:{}:{}m",
                detail % 256,
                (detail >> 8) % 256,
                (detail >> 16) % 256,
                (detail >> 24) % 256
            ),
        ),
        50..60 => put(out, format_args!("\x1b[58;5;{}m\x1b[2m", detail % 256)),
        60..80 => out.extend_from_slice(b"\x1b[m\x1b[10A\x1b[3E\x1b[2K"),
        _ => out.extend_from_slice(b"\x1b[39m\x1b[10`a\x1b[100b\x1b[?1l"),
    }
}

/// Combining marks `combining_unique` draws from: U+0300..U+036F.
const COMBINING_MARKS: usize = 112;

/// One cell per unit: `a` and three combining marks that, read as base-112 digits of the
/// unit number, make every triple unique for the first 112^3 units. The line wraps by itself.
fn combining_unique(target: usize) -> Vec<u8> {
    build(target, b"", |i, out| {
        out.push(b'a');
        let mut rest = i;
        for _ in 0..3 {
            let mark = char::from_u32(0x300 + (rest % COMBINING_MARKS) as u32);
            rest /= COMBINING_MARKS;
            if let Some(mark) = mark {
                let mut bytes = [0; 4];
                out.extend_from_slice(mark.encode_utf8(&mut bytes).as_bytes());
            }
        }
    })
}

/// Bytes in the payload of one `long_osc` string.
const LONG_OSC_PAYLOAD: usize = 8000;

/// An 8000-byte OSC 6 string that no terminal acts on, then a short visible line.
fn long_osc(target: usize) -> Vec<u8> {
    build(target, b"", |i, out| {
        out.extend_from_slice(b"\x1b]6;");
        out.extend((0..LONG_OSC_PAYLOAD).map(|n| letter(pick((i as u64) << 16 | n as u64, 26))));
        out.push(0x07);
        put(out, format_args!("osc {i}\r\n"));
    })
}

/// `symbols` and `vim_session` from github.com/alacritty/vtebench, unchanged (see the README
/// beside them). Both mix CR LF with bare LFs (2 in `symbols`, 77 in `vim_session`) and are fed as they are.
const VTEBENCH_SYMBOLS: &[u8] = include_bytes!("../data/vtebench/symbols");
const VTEBENCH_VIM_SESSION: &[u8] = include_bytes!("../data/vtebench/vim_session");

fn unicode_symbols(target: usize) -> Vec<u8> {
    build(target, b"", |_, out| {
        out.extend_from_slice(VTEBENCH_SYMBOLS);
    })
}

fn vim_session(target: usize) -> Vec<u8> {
    build(target, b"", |_, out| {
        out.extend_from_slice(VTEBENCH_VIM_SESSION);
    })
}

/// Which streams `fixtures` writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FixtureSet {
    /// The fourteen release streams.
    Release,
    /// The release streams plus the compare-only ones, in corpus order.
    Corpus,
}

/// Writes `DIR/<name>.vt` for every fixture of `set` (`mib` MiB each) and returns one report
/// line per file.
pub(crate) fn write_fixtures(dir: &Path, mib: usize, set: FixtureSet) -> Result<Vec<String>> {
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let compare: &[Fixture] = match set {
        FixtureSet::Release => &[],
        FixtureSet::Corpus => &COMPARE_FIXTURES,
    };
    let mut lines = Vec::with_capacity(FIXTURES.len() + compare.len());
    for fixture in FIXTURES.iter().chain(compare) {
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
    use oneterm_vt::parser::{Dispatch, OscParams, Params, Parser, StringTerm};

    use super::*;

    fn make(name: &str, target: usize) -> Vec<u8> {
        let fixture = FIXTURES
            .iter()
            .chain(&COMPARE_FIXTURES)
            .find(|f| f.name == name)
            .unwrap();
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
                "osc_9_7",
                "grapheme_clusters",
                "hyperlinks",
                "mode_churn",
                "sixel_large"
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
        for fixture in FIXTURES.iter().chain(&COMPARE_FIXTURES) {
            let first = (fixture.make)(100_000);
            assert_eq!(first, (fixture.make)(100_000), "{}", fixture.name);
            // Unit sizes vary (SGR numbers grow, URIs differ); 4096 covers every unit but a Sixel image.
            let longest_unit = (fixture.make)(1).len().max(4096);
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
        fn csi(&mut self, _: &Params, _: &[u8], ignore: bool, _: u8) {
            self.csis += 1;
            self.flagged += usize::from(ignore);
        }
        fn osc(&mut self, _: Option<u32>, _: &OscParams<'_>, _: StringTerm, truncated: bool) {
            self.oscs += 1;
            self.flagged += usize::from(truncated);
        }
        // `long_osc` is OSC 6; a terminal that truncates it past 2 KiB still ignores it, so
        // the parser check lets it through whole.
        fn osc_allows_large(&self, code: u32) -> bool {
            code == 6
        }
        fn dcs_hook(&mut self, _: &Params, _: &[u8], ignore: bool, _: u8) {
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
        for fixture in FIXTURES.iter().chain(&COMPARE_FIXTURES) {
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
        let links = tally("hyperlinks");
        assert_eq!(links.oscs, 4 * links.linefeeds);
        let large = tally("sixel_large");
        assert_eq!(large.hooks, large.unhooks);
        assert_eq!(large.hooks, large.linefeeds);
        assert!(tally("grapheme_clusters").printed > 1000);
        assert!(tally("mode_churn").csis > 100);
    }

    #[test]
    fn new_fixtures_start_with_their_own_setup() {
        assert!(make("grapheme_clusters", 1).starts_with(b"\x1b[?2027h"));
        assert!(make("hyperlinks", 1).starts_with(b"see \x1b]8;"));
        assert!(make("mode_churn", 1).starts_with(b"\x1b[?1049h"));
        assert!(make("sixel_large", 1).starts_with(b"\x1bPq\"1;1;1024;768#0;2;"));
    }

    #[test]
    fn link_uris_stay_inline_and_clean() {
        for k in 0..LINK_POOL {
            let uri = link_uri(k);
            assert!((200..=1000).contains(&uri.len()), "link {k}: {}", uri.len());
            assert!(
                uri.bytes().all(|b| b.is_ascii_graphic() && b != b';'),
                "link {k}"
            );
        }
        assert_ne!(link_uri(1), link_uri(2));
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
        let lines = write_fixtures(&dir, 1, FixtureSet::Release).unwrap();
        assert_eq!(lines.len(), FIXTURES.len());
        for fixture in &FIXTURES {
            let bytes = fs::read(dir.join(format!("{}.vt", fixture.name))).unwrap();
            assert!(bytes.len() >= 1 << 20);
        }
    }

    #[test]
    fn select_none_stays_on_the_release_fixtures_and_names_find_both() {
        assert_eq!(select(None).unwrap().len(), 14);
        let compare = select(Some("csi_mix")).unwrap();
        assert_eq!(compare[0].name, "csi_mix");
        let release = select(Some("plain_ascii")).unwrap();
        assert_eq!(release[0].name, "plain_ascii");
    }

    #[test]
    fn names_are_unique_across_both_tables_and_in_corpus_order() {
        let names: Vec<&str> = FIXTURES
            .iter()
            .chain(&COMPARE_FIXTURES)
            .map(|f| f.name)
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 23);
        assert_eq!(
            names[14..],
            [
                "cursor_motion",
                "light_cells",
                "short_lines",
                "scroll_region_small",
                "csi_mix",
                "combining_unique",
                "long_osc",
                "unicode_symbols",
                "vim_session"
            ]
        );
    }

    #[test]
    fn compare_fixtures_first_unit_matches_the_table() {
        assert!(make("cursor_motion", 1).starts_with(b"\x1b[1;1HA\x1b[1;2HA"));
        let light = make("light_cells", 1);
        assert_eq!(
            light,
            [b"\x1b[?1049h\x1b[H".as_slice(), &[b'a'; 7200]].concat()
        );
        assert_eq!(make("short_lines", 1), b"y\r\n");
        assert_eq!(
            make("scroll_region_small", 1),
            b"\x1b[?1049h\x1b[1;22ry\r\n"
        );
        let long = make("long_osc", 1);
        assert!(long.starts_with(b"\x1b]6;"));
        assert!(long.ends_with(b"\x07osc 0\r\n"));
        assert_eq!(long.len(), 4 + LONG_OSC_PAYLOAD + 1 + b"osc 0\r\n".len());
        assert!(
            long[4..4 + LONG_OSC_PAYLOAD]
                .iter()
                .all(u8::is_ascii_lowercase)
        );
    }

    #[test]
    fn cursor_motion_unit_visits_every_cell_once() {
        let cells = spiral_cells(COLUMNS, LINES);
        assert_eq!(cells.len(), COLUMNS * LINES);
        let mut seen = vec![false; COLUMNS * LINES];
        for &(row, col) in &cells {
            assert!(!std::mem::replace(&mut seen[row * COLUMNS + col], true));
        }
        // Every step stays inside a ring: neighbours except where a ring ends and the next begins.
        let unit = make("cursor_motion", 1);
        let cups = unit.windows(2).filter(|w| w == b"\x1b[").count();
        assert_eq!(cups, cells.len());
        assert_eq!(&unit[unit.len() - 1..], b"A");
    }

    #[test]
    fn spiral_handles_thin_grids() {
        for (columns, lines) in [(1, 1), (1, 5), (5, 1), (2, 2), (3, 7), (7, 3)] {
            let mut cells = spiral_cells(columns, lines);
            assert_eq!(cells.len(), columns * lines, "{columns}x{lines}");
            cells.sort_unstable();
            cells.dedup();
            assert_eq!(cells.len(), columns * lines, "{columns}x{lines}");
        }
    }

    #[test]
    fn combining_unique_units_are_pairwise_distinct() {
        let stream = make("combining_unique", 7 * 1000);
        let units: Vec<&[u8]> = stream
            .split_inclusive(|&b| b == b'a')
            .filter(|u| u.len() > 1)
            .collect();
        assert!(units.len() >= 1000);
        let mut firsts: Vec<&[u8]> = units[..1000].to_vec();
        firsts.sort_unstable();
        firsts.dedup();
        assert_eq!(firsts.len(), 1000);
    }

    /// Which of the seven `csi_mix` branches produced `unit`.
    fn csi_branch(unit: &[u8]) -> usize {
        let has = |needle: &[u8]| unit.windows(needle.len()).any(|w| w == needle);
        match () {
            () if !has(b"\x1b") => 0,
            () if has(b"\x1b[?1h") => 1,
            () if has(b"4:3;31m") => 2,
            () if has(b"38:5:") => 3,
            () if has(b"58;5;") => 4,
            () if has(b"\x1b[10A") => 5,
            () => 6,
        }
    }

    #[test]
    fn csi_mix_uses_all_seven_branches_roughly_by_weight() {
        let mut counts = [0usize; 7];
        let mut out = Vec::new();
        for i in 0..10_000 {
            out.clear();
            csi_mix_unit(i, &mut out);
            counts[csi_branch(&out)] += 1;
        }
        let weights = [10, 20, 10, 10, 10, 20, 20];
        for (branch, weight) in weights.iter().enumerate() {
            let expected = weight * 100;
            assert!(
                counts[branch] > expected * 7 / 10 && counts[branch] < expected * 13 / 10,
                "branch {branch}: {} of 10000, expected about {expected}",
                counts[branch]
            );
        }
    }

    #[test]
    fn vendored_streams_are_whole_units_and_their_line_ends_are_known() {
        // vtebench feeds these through a PTY that may add the CR (ONLCR); the files hold bare
        // LFs (2 and 77) that no engine here translates, so all of them see the same bytes.
        let bare_lfs = |file: &[u8]| {
            (0..file.len())
                .filter(|&n| file[n] == b'\n' && (n == 0 || file[n - 1] != b'\r'))
                .count()
        };
        assert_eq!(make("unicode_symbols", 1), VTEBENCH_SYMBOLS);
        assert_eq!(make("vim_session", 1), VTEBENCH_VIM_SESSION);
        assert_eq!(bare_lfs(VTEBENCH_SYMBOLS), 2);
        assert_eq!(bare_lfs(VTEBENCH_VIM_SESSION), 77);
        assert_eq!(VTEBENCH_SYMBOLS.len(), 138_296);
        assert_eq!(VTEBENCH_VIM_SESSION.len(), 178_345);
    }

    #[test]
    fn every_sync_update_is_closed_in_each_unit() {
        // vt100 has no flush: a `?2026h` left open would hold bytes back until the end.
        for fixture in FIXTURES.iter().chain(&COMPARE_FIXTURES) {
            let unit = (fixture.make)(1);
            let mut open = false;
            for window in unit.windows(8) {
                if window == b"\x1b[?2026h" {
                    assert!(!open, "{}: nested ?2026h", fixture.name);
                    open = true;
                } else if window == b"\x1b[?2026l" {
                    open = false;
                }
            }
            assert!(!open, "{}: ?2026h left open", fixture.name);
        }
    }

    #[test]
    fn write_fixtures_adds_the_compare_streams_only_on_request() {
        struct RemoveOnDrop(std::path::PathBuf);
        impl Drop for RemoveOnDrop {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let base = std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.tmp/fixtures-test"
        ))
        .join("write_fixtures_adds_the_compare_streams_only_on_request");
        let _guard = RemoveOnDrop(base.clone());
        let count = |dir: &Path| fs::read_dir(dir).unwrap().count();
        assert_eq!(
            write_fixtures(&base.join("release"), 1, FixtureSet::Release)
                .unwrap()
                .len(),
            14
        );
        assert_eq!(count(&base.join("release")), 14);
        assert_eq!(
            write_fixtures(&base.join("corpus"), 1, FixtureSet::Corpus)
                .unwrap()
                .len(),
            23
        );
        assert_eq!(count(&base.join("corpus")), 23);
    }
}

#[cfg(test)]
#[path = "fixture_effects_tests.rs"]
mod fixture_effects_tests;
