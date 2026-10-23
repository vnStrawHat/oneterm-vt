use super::*;

const _: () = assert!(size_of::<Style>() <= 16);

/// Bits 27..32 must never be set by any setter.
const RESERVED: u64 = 0b11111 << 27;

#[test]
fn empty_cell_is_a_default_space() {
    assert_eq!(Cell::EMPTY.to_bits(), 0);
    assert_eq!(Cell::EMPTY.content(), CellContent::Scalar(' '));
    assert_eq!(Cell::EMPTY.width(), CellWidth::Narrow);
    assert_eq!(Cell::EMPTY.semantic(), Semantic::None);
    assert!(!Cell::EMPTY.is_protected());
    assert_eq!(Cell::EMPTY.style_id(), StyleId::DEFAULT);
    assert_eq!(Cell::EMPTY.extras_id(), ExtrasId::NONE);
    assert_eq!(Cell::default(), Cell::EMPTY);
}

#[test]
fn space_and_nul_have_one_encoding() {
    assert_eq!(
        Cell::EMPTY.with_content(CellContent::Scalar(' ')),
        Cell::EMPTY
    );
    let nul = Cell::EMPTY.with_content(CellContent::Scalar('\0'));
    assert_eq!(nul.content(), CellContent::Scalar(' '));
    assert_eq!(nul, Cell::EMPTY);
}

#[test]
fn content_edges_round_trip() {
    let contents = [
        CellContent::Scalar('\u{10FFFF}'),
        CellContent::Scalar('a'),
        CellContent::Scalar('\u{1}'),
        CellContent::Grapheme(GraphemeId(0)),
        CellContent::Grapheme(GraphemeId(GRAPHEME_ID_LIMIT - 1)),
    ];
    for content in contents {
        assert_eq!(Cell::EMPTY.with_content(content).content(), content);
    }
}

#[test]
fn out_of_range_grapheme_id_falls_back() {
    let base = Cell::EMPTY
        .with_width(CellWidth::Wide)
        .with_style(StyleId(7));
    for id in [GRAPHEME_ID_LIMIT, u32::MAX] {
        let cell = base.with_content(CellContent::Grapheme(GraphemeId(id)));
        assert_eq!(cell.content(), CellContent::Grapheme(GraphemeId(0)));
        assert_eq!(cell.width(), CellWidth::Wide);
        assert_eq!(cell.style_id(), StyleId(7));
    }
}

#[test]
fn bit_layout_is_the_documented_one() {
    let e = Cell::EMPTY;
    assert_eq!(e.with_style(StyleId(0xABCD)).to_bits(), 0xABCD << 32);
    assert_eq!(e.with_extras(ExtrasId(0xABCD)).to_bits(), 0xABCD << 48);
    assert_eq!(e.with_width(CellWidth::Wide).to_bits(), 1 << 22);
    assert_eq!(e.with_width(CellWidth::WideSpacer).to_bits(), 2 << 22);
    assert_eq!(
        e.with_width(CellWidth::LeadingWideSpacer).to_bits(),
        3 << 22
    );
    assert_eq!(e.with_semantic(Semantic::Prompt).to_bits(), 1 << 24);
    assert_eq!(e.with_semantic(Semantic::Output).to_bits(), 3 << 24);
    assert_eq!(e.with_protected(true).to_bits(), 1 << 26);
    assert_eq!(
        e.with_content(CellContent::Grapheme(GraphemeId(5)))
            .to_bits(),
        5 | 1 << 21
    );
    assert_eq!(e.with_content(CellContent::Scalar('A')).to_bits(), 0x41);
}

#[test]
fn spacer_roles() {
    assert!(!CellWidth::Narrow.is_spacer());
    assert!(!CellWidth::Wide.is_spacer());
    assert!(CellWidth::WideSpacer.is_spacer());
    assert!(CellWidth::LeadingWideSpacer.is_spacer());
}

#[test]
fn text_char_reads_scalar_or_cluster_base() {
    let mut arena = GraphemeArena::default();
    let id = arena.intern("e\u{301}");
    let scalar = Cell::EMPTY.with_content(CellContent::Scalar('x'));
    let cluster = Cell::EMPTY.with_content(CellContent::Grapheme(id));
    let unknown = Cell::EMPTY.with_content(CellContent::Grapheme(GraphemeId(500)));
    assert_eq!(scalar.text_char(&arena), 'x');
    assert_eq!(cluster.text_char(&arena), 'e');
    assert_eq!(unknown.text_char(&arena), ' ');
}

#[test]
fn default_style_is_foreground_on_background() {
    assert_eq!(Style::DEFAULT.fg, Color::Named(NamedColor::Foreground));
    assert_eq!(Style::DEFAULT.bg, Color::Named(NamedColor::Background));
    assert_eq!(Style::DEFAULT.underline_color, None);
    assert_eq!(Style::DEFAULT.attrs, Attrs::empty());
    assert_eq!(Style::default(), Style::DEFAULT);
}

#[test]
fn all_underlines_is_the_five_underline_flags() {
    let five = Attrs::UNDERLINE
        | Attrs::DOUBLE_UNDERLINE
        | Attrs::UNDERCURL
        | Attrs::DOTTED_UNDERLINE
        | Attrs::DASHED_UNDERLINE;
    assert_eq!(Attrs::ALL_UNDERLINES, five);
    assert_eq!(Attrs::ALL_UNDERLINES.bits(), 0x1F00);
}

mod props {
    use proptest::prelude::*;

    use super::*;

    fn any_width() -> impl Strategy<Value = CellWidth> {
        prop_oneof![
            Just(CellWidth::Narrow),
            Just(CellWidth::Wide),
            Just(CellWidth::WideSpacer),
            Just(CellWidth::LeadingWideSpacer),
        ]
    }

    fn any_semantic() -> impl Strategy<Value = Semantic> {
        prop_oneof![
            Just(Semantic::None),
            Just(Semantic::Prompt),
            Just(Semantic::Input),
            Just(Semantic::Output),
        ]
    }

    fn any_content() -> impl Strategy<Value = CellContent> {
        prop_oneof![
            any::<char>().prop_map(CellContent::Scalar),
            (0..GRAPHEME_ID_LIMIT).prop_map(|id| CellContent::Grapheme(GraphemeId(id))),
        ]
    }

    fn build(
        content: CellContent,
        width: CellWidth,
        semantic: Semantic,
        protected: bool,
        style: u16,
        extras: u16,
    ) -> Cell {
        Cell::EMPTY
            .with_content(content)
            .with_width(width)
            .with_semantic(semantic)
            .with_protected(protected)
            .with_style(StyleId(style))
            .with_extras(ExtrasId(extras))
    }

    fn expected(content: CellContent) -> CellContent {
        match content {
            CellContent::Scalar('\0') => CellContent::Scalar(' '),
            other => other,
        }
    }

    proptest! {
        #[test]
        fn scalar_cells_round_trip(
            c in any::<char>(),
            width in any_width(),
            semantic in any_semantic(),
            protected in any::<bool>(),
            style in any::<u16>(),
            extras in any::<u16>(),
        ) {
            let content = CellContent::Scalar(c);
            let cell = build(content, width, semantic, protected, style, extras);
            prop_assert_eq!(cell.content(), expected(content));
            prop_assert_eq!(cell.width(), width);
            prop_assert_eq!(cell.semantic(), semantic);
            prop_assert_eq!(cell.is_protected(), protected);
            prop_assert_eq!(cell.style_id(), StyleId(style));
            prop_assert_eq!(cell.extras_id(), ExtrasId(extras));
            prop_assert_eq!(cell.to_bits() & RESERVED, 0);
        }

        #[test]
        fn grapheme_cells_round_trip(
            id in 0..GRAPHEME_ID_LIMIT,
            width in any_width(),
            semantic in any_semantic(),
            protected in any::<bool>(),
            style in any::<u16>(),
            extras in any::<u16>(),
        ) {
            let content = CellContent::Grapheme(GraphemeId(id));
            let cell = build(content, width, semantic, protected, style, extras);
            prop_assert_eq!(cell.content(), content);
            prop_assert_eq!(cell.width(), width);
            prop_assert_eq!(cell.semantic(), semantic);
            prop_assert_eq!(cell.is_protected(), protected);
            prop_assert_eq!(cell.style_id(), StyleId(style));
            prop_assert_eq!(cell.extras_id(), ExtrasId(extras));
            prop_assert_eq!(cell.to_bits() & RESERVED, 0);
        }

        #[test]
        fn setters_touch_only_their_field(
            content in any_content(),
            width in any_width(),
            semantic in any_semantic(),
            protected in any::<bool>(),
            style in any::<u16>(),
            extras in any::<u16>(),
            new_content in any_content(),
            new_width in any_width(),
            new_semantic in any_semantic(),
            new_protected in any::<bool>(),
            new_style in any::<u16>(),
            new_extras in any::<u16>(),
            which in 0..6u8,
        ) {
            let cell = build(content, width, semantic, protected, style, extras);
            let changed = match which {
                0 => cell.with_content(new_content),
                1 => cell.with_width(new_width),
                2 => cell.with_semantic(new_semantic),
                3 => cell.with_protected(new_protected),
                4 => cell.with_style(StyleId(new_style)),
                _ => cell.with_extras(ExtrasId(new_extras)),
            };
            if which != 0 {
                prop_assert_eq!(changed.content(), cell.content());
            }
            if which != 1 {
                prop_assert_eq!(changed.width(), cell.width());
            }
            if which != 2 {
                prop_assert_eq!(changed.semantic(), cell.semantic());
            }
            if which != 3 {
                prop_assert_eq!(changed.is_protected(), cell.is_protected());
            }
            if which != 4 {
                prop_assert_eq!(changed.style_id(), cell.style_id());
            }
            if which != 5 {
                prop_assert_eq!(changed.extras_id(), cell.extras_id());
            }
        }
    }
}
