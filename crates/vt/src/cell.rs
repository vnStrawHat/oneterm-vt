//! What one grid position stores.
//!
//! A [`Cell`] is 8 bytes with no pointer. Everything rare (style, hyperlink, image, long
//! cluster) is an id into the terminal's [`Interner`](crate::intern::Interner), so uniform
//! text stores a single style no matter how many cells use it.

use std::fmt;

use bitflags::bitflags;

use crate::intern::{ExtrasId, GRAPHEME_ID_LIMIT, GraphemeArena, GraphemeId, StyleId};

const CONTENT_MASK: u64 = (1 << 21) - 1;
const GRAPHEME_BIT: u64 = 1 << 21;
const WIDTH_SHIFT: u32 = 22;
const WIDTH_MASK: u64 = 0b11;
const SEMANTIC_SHIFT: u32 = 24;
const SEMANTIC_MASK: u64 = 0b11;
const PROTECTED_BIT: u64 = 1 << 26;
const STYLE_SHIFT: u32 = 32;
const EXTRAS_SHIFT: u32 = 48;
const ID_MASK: u64 = 0xFFFF;

const _: () = assert!(size_of::<Cell>() == 8);

/// One grid position: content, column role, semantic zone, protection, style id and extras id.
///
/// Equal cells are bitwise equal: a blank has exactly one encoding (see
/// [`Cell::with_content`]). Ids are only meaningful against the
/// [`Interner`](crate::intern::Interner) of the terminal that produced them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(transparent)]
pub struct Cell(u64);

/// What a cell shows: one scalar value, or a multi-scalar cluster stored in the grapheme arena.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CellContent {
    /// A single Unicode scalar. A blank cell reads as `' '`.
    Scalar(char),
    /// A grapheme cluster, resolved through [`GraphemeArena`].
    Grapheme(GraphemeId),
}

/// A cell's role in a double-width pair.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum CellWidth {
    /// One column, not part of a pair.
    #[default]
    Narrow,
    /// The left half of a double-width glyph; the glyph is stored here.
    Wide,
    /// The right half of a double-width glyph; holds no content of its own.
    WideSpacer,
    /// A blank in the last column standing in for a wide glyph that wrapped to the next line.
    LeadingWideSpacer,
}

impl CellWidth {
    /// Whether the cell is a placeholder half of a pair rather than a glyph.
    pub const fn is_spacer(self) -> bool {
        matches!(self, CellWidth::WideSpacer | CellWidth::LeadingWideSpacer)
    }
}

/// The shell-integration zone (OSC 133) a cell was written in.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Semantic {
    /// Not inside a marked zone.
    #[default]
    None,
    /// Prompt text (between marks A and B).
    Prompt,
    /// User input (between marks B and C).
    Input,
    /// Command output (between marks C and D).
    Output,
}

impl Cell {
    /// A blank, narrow, unprotected cell with the default style and no extras.
    ///
    /// `Grapheme(GraphemeId(0))` also renders blank but is not bit-equal to `EMPTY`; blank
    /// predicates must treat it as blank.
    pub const EMPTY: Cell = Cell(0);

    /// Returns the cell's content. The blank encoding reads back as `' '`.
    pub fn content(self) -> CellContent {
        // The cast keeps at most 21 bits.
        let value = (self.0 & CONTENT_MASK) as u32;
        if self.0 & GRAPHEME_BIT != 0 {
            return CellContent::Grapheme(GraphemeId(value));
        }
        // Stored scalars come from a `char`, so the fallback only guards a hand-built word.
        let scalar = if value == 0 {
            ' '
        } else {
            char::from_u32(value).unwrap_or(' ')
        };
        CellContent::Scalar(scalar)
    }

    /// Returns the cell's column role.
    pub fn width(self) -> CellWidth {
        match (self.0 >> WIDTH_SHIFT) & WIDTH_MASK {
            0 => CellWidth::Narrow,
            1 => CellWidth::Wide,
            2 => CellWidth::WideSpacer,
            _ => CellWidth::LeadingWideSpacer,
        }
    }

    /// Returns the cell's shell-integration zone.
    pub fn semantic(self) -> Semantic {
        match (self.0 >> SEMANTIC_SHIFT) & SEMANTIC_MASK {
            0 => Semantic::None,
            1 => Semantic::Prompt,
            2 => Semantic::Input,
            _ => Semantic::Output,
        }
    }

    /// Whether the cell is protected from selective erase (DECSCA).
    pub fn is_protected(self) -> bool {
        self.0 & PROTECTED_BIT != 0
    }

    /// Returns the id of the cell's style in the terminal's style table.
    pub fn style_id(self) -> StyleId {
        // Masked to 16 bits.
        StyleId(((self.0 >> STYLE_SHIFT) & ID_MASK) as u16)
    }

    /// Returns the id of the cell's extras (hyperlink, image) in the terminal's extras table.
    pub fn extras_id(self) -> ExtrasId {
        // Masked to 16 bits.
        ExtrasId(((self.0 >> EXTRAS_SHIFT) & ID_MASK) as u16)
    }

    /// Returns the cell with new content.
    ///
    /// `' '` and `'\0'` are both stored as the blank encoding, so a blank is always equal to
    /// [`Cell::EMPTY`]'s content. A grapheme id at or above the arena limit is stored as
    /// `GraphemeId(0)`, which resolves to `" "`.
    #[must_use]
    pub fn with_content(self, content: CellContent) -> Cell {
        let bits = match content {
            CellContent::Scalar(' ' | '\0') => 0,
            CellContent::Scalar(c) => u64::from(c),
            CellContent::Grapheme(id) if id.0 >= GRAPHEME_ID_LIMIT => GRAPHEME_BIT,
            CellContent::Grapheme(id) => GRAPHEME_BIT | u64::from(id.0),
        };
        self.with_field(CONTENT_MASK | GRAPHEME_BIT, 0, bits)
    }

    /// Returns the cell with a new column role.
    #[must_use]
    pub fn with_width(self, width: CellWidth) -> Cell {
        self.with_field(WIDTH_MASK, WIDTH_SHIFT, width as u64)
    }

    /// Returns the cell with a new shell-integration zone.
    #[must_use]
    pub fn with_semantic(self, semantic: Semantic) -> Cell {
        self.with_field(SEMANTIC_MASK, SEMANTIC_SHIFT, semantic as u64)
    }

    /// Returns the cell with the protection flag set or cleared.
    #[must_use]
    pub fn with_protected(self, protected: bool) -> Cell {
        self.with_field(PROTECTED_BIT, 0, if protected { PROTECTED_BIT } else { 0 })
    }

    /// Returns the cell with a new style id.
    #[must_use]
    pub fn with_style(self, id: StyleId) -> Cell {
        self.with_field(ID_MASK, STYLE_SHIFT, u64::from(id.0))
    }

    /// Returns the cell with a new extras id.
    #[must_use]
    pub fn with_extras(self, id: ExtrasId) -> Cell {
        self.with_field(ID_MASK, EXTRAS_SHIFT, u64::from(id.0))
    }

    /// Returns the first scalar the cell shows: its scalar, or the first scalar of its
    /// cluster. Empty or unknown clusters give `' '`.
    pub fn text_char(self, graphemes: &GraphemeArena) -> char {
        match self.content() {
            CellContent::Scalar(c) => c,
            CellContent::Grapheme(id) => graphemes.resolve(id).chars().next().unwrap_or(' '),
        }
    }

    /// Returns the raw 64-bit word, for tests and snapshots. The layout may change between
    /// versions.
    pub fn to_bits(self) -> u64 {
        self.0
    }

    /// Replaces the bits under `mask << shift` with `value << shift`.
    fn with_field(self, mask: u64, shift: u32, value: u64) -> Cell {
        Cell((self.0 & !(mask << shift)) | ((value & mask) << shift))
    }
}

impl fmt::Debug for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cell")
            .field("content", &self.content())
            .field("width", &self.width())
            .field("semantic", &self.semantic())
            .field("protected", &self.is_protected())
            .field("style", &self.style_id())
            .field("extras", &self.extras_id())
            .finish()
    }
}

bitflags! {
    /// Text attributes. Which SGR parameter sets which flag is decided by the print path.
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct Attrs: u16 {
        /// Bold (SGR 1).
        const BOLD = 1 << 0;
        /// Dim (SGR 2).
        const DIM = 1 << 1;
        /// Italic (SGR 3).
        const ITALIC = 1 << 2;
        /// Swap foreground and background (SGR 7).
        const INVERSE = 1 << 3;
        /// Hidden (SGR 8).
        const HIDDEN = 1 << 4;
        /// Strikeout (SGR 9).
        const STRIKEOUT = 1 << 5;
        /// Slow blink (SGR 5).
        const BLINK_SLOW = 1 << 6;
        /// Fast blink (SGR 6).
        const BLINK_FAST = 1 << 7;
        /// Single underline (SGR 4, 4:1).
        const UNDERLINE = 1 << 8;
        /// Double underline (SGR 21, 4:2).
        const DOUBLE_UNDERLINE = 1 << 9;
        /// Curly underline (SGR 4:3).
        const UNDERCURL = 1 << 10;
        /// Dotted underline (SGR 4:4).
        const DOTTED_UNDERLINE = 1 << 11;
        /// Dashed underline (SGR 4:5).
        const DASHED_UNDERLINE = 1 << 12;
        /// Overline (SGR 53).
        const OVERLINE = 1 << 13;
        /// The union of the five underline styles.
        const ALL_UNDERLINES = Self::UNDERLINE.bits()
            | Self::DOUBLE_UNDERLINE.bits()
            | Self::UNDERCURL.bits()
            | Self::DOTTED_UNDERLINE.bits()
            | Self::DASHED_UNDERLINE.bits();
    }
}

/// A colour as the byte stream named it. Resolving it to pixels (theme, OSC 4/10/11
/// overrides) is the embedder's job.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Color {
    /// A named slot.
    Named(NamedColor),
    /// An index into the 256-colour palette.
    Palette(u8),
    /// A direct 24-bit colour.
    Rgb(Rgb),
}

/// A 24-bit colour.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Rgb {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

/// A named colour slot.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NamedColor {
    /// Palette 0.
    Black,
    /// Palette 1.
    Red,
    /// Palette 2.
    Green,
    /// Palette 3.
    Yellow,
    /// Palette 4.
    Blue,
    /// Palette 5.
    Magenta,
    /// Palette 6.
    Cyan,
    /// Palette 7.
    White,
    /// Palette 8.
    BrightBlack,
    /// Palette 9.
    BrightRed,
    /// Palette 10.
    BrightGreen,
    /// Palette 11.
    BrightYellow,
    /// Palette 12.
    BrightBlue,
    /// Palette 13.
    BrightMagenta,
    /// Palette 14.
    BrightCyan,
    /// Palette 15.
    BrightWhite,
    /// The default foreground (OSC 10).
    Foreground,
    /// The default background (OSC 11).
    Background,
    /// The cursor colour (OSC 12).
    Cursor,
    /// The bright variant of the default foreground.
    BrightForeground,
    /// The dim variant of the default foreground.
    DimForeground,
    /// Dim variant of black.
    DimBlack,
    /// Dim variant of red.
    DimRed,
    /// Dim variant of green.
    DimGreen,
    /// Dim variant of yellow.
    DimYellow,
    /// Dim variant of blue.
    DimBlue,
    /// Dim variant of magenta.
    DimMagenta,
    /// Dim variant of cyan.
    DimCyan,
    /// Dim variant of white.
    DimWhite,
}

/// The visual style of a cell: colours and attributes. It is the key of the style table.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Style {
    /// Foreground colour.
    pub fg: Color,
    /// Background colour.
    pub bg: Color,
    /// Underline colour (SGR 58/59); `None` follows the foreground.
    pub underline_color: Option<Color>,
    /// Text attributes.
    pub attrs: Attrs,
}

impl Style {
    /// Default foreground on default background, no attributes.
    pub const DEFAULT: Style = Style {
        fg: Color::Named(NamedColor::Foreground),
        bg: Color::Named(NamedColor::Background),
        underline_color: None,
        attrs: Attrs::empty(),
    };
}

impl Default for Style {
    fn default() -> Self {
        Style::DEFAULT
    }
}

#[cfg(test)]
#[path = "cell_tests.rs"]
mod cell_tests;
