//! The colours a renderer resolves cell colours against.

use crate::cell::{Color, NamedColor, Rgb};

/// The 16 ANSI colours of a default palette, as `0xRRGGBB`.
const ANSI: [u32; 16] = [
    0x00_00_00, 0xcd_00_00, 0x00_cd_00, 0xcd_cd_00, 0x00_00_ee, 0xcd_00_cd, 0x00_cd_cd, 0xe5_e5_e5,
    0x7f_7f_7f, 0xff_00_00, 0x00_ff_00, 0xff_ff_00, 0x5c_5c_ff, 0xff_00_ff, 0x00_ff_ff, 0xff_ff_ff,
];
/// The channel levels of the 6x6x6 colour cube.
const CUBE_LEVELS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];

const DEFAULT_FOREGROUND: Rgb = Rgb {
    r: 0xd8,
    g: 0xd8,
    b: 0xd8,
};
const DEFAULT_BACKGROUND: Rgb = Rgb {
    r: 0x18,
    g: 0x18,
    b: 0x18,
};

/// The colours a renderer draws with: the 256 indexed colours and the default foreground,
/// background and cursor.
///
/// [`SnapshotState::map_colors`](crate::SnapshotState::map_colors) resolves every cell colour
/// against one. The embedder's theme goes in through
/// [`Terminal::set_theme_colors`](crate::Terminal::set_theme_colors); programs override single
/// slots with OSC 4, 10, 11 and 12, and
/// [`Terminal::palette`](crate::Terminal::palette) returns the two merged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Palette {
    /// Entries 0..16 are the ANSI colours, 16..232 the 6x6x6 cube, 232..256 the greys.
    pub indexed: [Rgb; 256],
    /// The default foreground.
    pub foreground: Rgb,
    /// The default background.
    pub background: Rgb,
    /// The cursor colour.
    pub cursor: Rgb,
    /// The bright variant of the foreground; `None` uses `foreground`.
    pub bright_foreground: Option<Rgb>,
    /// The dim variant of the foreground; `None` uses [`dim`](Self::dim) of `foreground`.
    pub dim_foreground: Option<Rgb>,
}

impl Default for Palette {
    fn default() -> Palette {
        Palette::new()
    }
}

fn rgb(hex: u32) -> Rgb {
    let [_, r, g, b] = hex.to_be_bytes();
    Rgb { r, g, b }
}

/// The default colour of indexed slot `index`.
fn default_entry(index: u8) -> Rgb {
    match index {
        0..=15 => ANSI
            .get(usize::from(index))
            .map_or(DEFAULT_BACKGROUND, |hex| rgb(*hex)),
        16..=231 => {
            let cube = index - 16;
            let level = |step: u8| CUBE_LEVELS.get(usize::from(step)).copied().unwrap_or(0);
            Rgb {
                r: level(cube / 36),
                g: level(cube / 6 % 6),
                b: level(cube % 6),
            }
        }
        // The last grey is `8 + 10 * 23 = 238`, which fits a byte.
        232..=255 => {
            let grey = 8 + 10 * (index - 232);
            Rgb {
                r: grey,
                g: grey,
                b: grey,
            }
        }
    }
}

impl Palette {
    /// The default palette: the conventional ANSI colours, the standard cube and greys, a light
    /// grey foreground and cursor, and a near-black background.
    pub fn new() -> Palette {
        Palette {
            indexed: std::array::from_fn(|index| {
                default_entry(u8::try_from(index).unwrap_or(u8::MAX))
            }),
            foreground: DEFAULT_FOREGROUND,
            background: DEFAULT_BACKGROUND,
            cursor: DEFAULT_FOREGROUND,
            bright_foreground: None,
            dim_foreground: None,
        }
    }

    /// The colour `color` stands for. An `Rgb` resolves to itself.
    pub fn resolve(&self, color: Color) -> Rgb {
        match color {
            Color::Rgb(rgb) => rgb,
            Color::Palette(index) => self.slot(usize::from(index)),
            Color::Named(name) => self.resolve_named(name),
        }
    }

    /// `color` mixed half and half with the background; odd channel sums round up.
    pub fn dim(&self, color: Rgb) -> Rgb {
        let mix = |own: u8, back: u8| {
            u8::try_from((u16::from(own) + u16::from(back)).div_ceil(2)).unwrap_or(u8::MAX)
        };
        Rgb {
            r: mix(color.r, self.background.r),
            g: mix(color.g, self.background.g),
            b: mix(color.b, self.background.b),
        }
    }

    fn slot(&self, index: usize) -> Rgb {
        self.indexed.get(index).copied().unwrap_or(self.foreground)
    }

    fn resolve_named(&self, name: NamedColor) -> Rgb {
        match name {
            NamedColor::Black => self.slot(0),
            NamedColor::Red => self.slot(1),
            NamedColor::Green => self.slot(2),
            NamedColor::Yellow => self.slot(3),
            NamedColor::Blue => self.slot(4),
            NamedColor::Magenta => self.slot(5),
            NamedColor::Cyan => self.slot(6),
            NamedColor::White => self.slot(7),
            NamedColor::BrightBlack => self.slot(8),
            NamedColor::BrightRed => self.slot(9),
            NamedColor::BrightGreen => self.slot(10),
            NamedColor::BrightYellow => self.slot(11),
            NamedColor::BrightBlue => self.slot(12),
            NamedColor::BrightMagenta => self.slot(13),
            NamedColor::BrightCyan => self.slot(14),
            NamedColor::BrightWhite => self.slot(15),
            NamedColor::Foreground => self.foreground,
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            NamedColor::BrightForeground => self.bright_foreground.unwrap_or(self.foreground),
            NamedColor::DimForeground => self
                .dim_foreground
                .unwrap_or_else(|| self.dim(self.foreground)),
            NamedColor::DimBlack => self.dim(self.slot(0)),
            NamedColor::DimRed => self.dim(self.slot(1)),
            NamedColor::DimGreen => self.dim(self.slot(2)),
            NamedColor::DimYellow => self.dim(self.slot(3)),
            NamedColor::DimBlue => self.dim(self.slot(4)),
            NamedColor::DimMagenta => self.dim(self.slot(5)),
            NamedColor::DimCyan => self.dim(self.slot(6)),
            NamedColor::DimWhite => self.dim(self.slot(7)),
        }
    }
}
