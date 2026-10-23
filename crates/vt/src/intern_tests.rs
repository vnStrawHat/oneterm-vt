use super::*;
use crate::cell::{Attrs, Color, Rgb};

/// One cluster of `len` scalars (`len >= 2`), unique per `n`.
fn unique_cluster(n: u32, len: usize) -> String {
    let base = char::from_u32(0x4E00 + n % 20_992).unwrap();
    let mark = char::from_u32(0x0300 + n / 20_992).unwrap();
    let mut cluster = String::from(base);
    cluster.push(mark);
    cluster.extend(std::iter::repeat_n('\u{301}', len - 2));
    cluster
}

fn rgb_style(n: u32) -> Style {
    Style {
        fg: Color::Rgb(Rgb {
            r: (n >> 16) as u8,
            g: (n >> 8) as u8,
            b: n as u8,
        }),
        ..Style::DEFAULT
    }
}

#[test]
fn defaults_are_id_zero() {
    let mut interner = Interner::default();
    assert_eq!(interner.style(Style::DEFAULT), StyleId::DEFAULT);
    assert_eq!(interner.extras(Extras::NONE), ExtrasId::NONE);
    assert_eq!(interner.styles.entries(), 1);
    assert_eq!(interner.extras.entries(), 1);
}

#[test]
fn equal_values_share_an_id() {
    let mut interner = Interner::default();
    let a = interner.style(rgb_style(1));
    let b = interner.style(rgb_style(1));
    assert_eq!(a, b);
    assert_eq!(interner.styles.entries(), 2);
}

#[test]
fn ids_are_stable() {
    let mut interner = Interner::default();
    let ids: Vec<StyleId> = (1..=1000).map(|n| interner.style(rgb_style(n))).collect();
    for (n, id) in (1..=1000).zip(&ids).collect::<Vec<_>>().into_iter().rev() {
        assert_eq!(interner.style(rgb_style(n)), *id);
        assert_eq!(*interner.resolve_style(*id), rgb_style(n));
    }
}

#[test]
fn unknown_ids_resolve_to_defaults() {
    let interner = Interner::default();
    assert_eq!(*interner.resolve_style(StyleId(999)), Style::DEFAULT);
    assert_eq!(*interner.resolve_extras(ExtrasId(999)), Extras::NONE);
}

#[test]
fn style_table_full_falls_back_and_counts() {
    let mut interner = Interner::default();
    for n in 1..TABLE_CAPACITY as u32 {
        interner.style(rgb_style(n));
    }
    assert_eq!(interner.styles.entries(), 65_536);
    assert_eq!(interner.styles.exhausted(), 0);
    assert_eq!(interner.style(rgb_style(1 << 20)), StyleId::DEFAULT);
    assert_eq!(interner.styles.exhausted(), 1);
    assert_eq!(interner.style(rgb_style(1)), StyleId(1));
    assert_eq!(interner.styles.exhausted(), 1);
}

#[test]
fn one_extras_entry_per_image() {
    let mut interner = Interner::default();
    let image = Extras {
        graphic: Some(GraphicId(7)),
        ..Extras::NONE
    };
    let first = interner.extras(image);
    for _ in 0..10_000 {
        assert_eq!(interner.extras(image), first);
    }
    assert_eq!(interner.extras.entries(), 2);
}

#[test]
fn grapheme_dedupe_and_reserved_zero() {
    let mut arena = GraphemeArena::default();
    let a = arena.intern("e\u{301}");
    assert_eq!(a, arena.intern("e\u{301}"));
    assert_ne!(a, GraphemeId(0));
    assert_eq!(arena.intern(""), GraphemeId(0));
    assert_eq!(arena.intern(" "), GraphemeId(0));
    assert_eq!(arena.resolve(GraphemeId(0)), " ");
    assert_eq!(arena.resolve(a), "e\u{301}");
    assert_eq!(arena.resolve(GraphemeId(77)), "");
    assert_eq!(arena.entries(), 2);
}

#[test]
fn grapheme_truncates_at_16_scalars() {
    let mut arena = GraphemeArena::default();
    let sixteen = "a".to_string() + &"\u{301}".repeat(15);
    let id = arena.intern(&sixteen);
    assert_eq!(arena.resolve(id), sixteen);
    assert_eq!(arena.truncated(), 0);

    let seventeen = sixteen.clone() + "\u{301}";
    let id = arena.intern(&seventeen);
    assert_eq!(arena.truncated(), 1);
    assert_eq!(arena.resolve(id), sixteen);

    let wide = "\u{1F600}".repeat(20);
    let id = arena.intern(&wide);
    assert_eq!(arena.truncated(), 2);
    assert_eq!(arena.resolve(id), "\u{1F600}".repeat(16));
}

#[test]
fn grapheme_full_falls_back_to_space() {
    let mut arena = GraphemeArena::with_limits(4, 1 << 20);
    for n in 0..3 {
        assert_ne!(arena.intern(&unique_cluster(n, 2)), GraphemeId(0));
    }
    assert_eq!(arena.exhausted(), 0);
    assert_eq!(arena.intern(&unique_cluster(3, 2)), GraphemeId(0));
    assert_eq!(arena.exhausted(), 1);
    assert_ne!(arena.intern(&unique_cluster(0, 2)), GraphemeId(0));
    assert_eq!(arena.exhausted(), 1);

    // 1 scalar for id 0 plus 3 clusters of 2 scalars is 7; an 8-scalar cap fits one more.
    let mut arena = GraphemeArena::with_limits(1 << 18, 8);
    assert_ne!(arena.intern(&unique_cluster(0, 2)), GraphemeId(0));
    assert_ne!(arena.intern(&unique_cluster(1, 2)), GraphemeId(0));
    assert_ne!(arena.intern(&unique_cluster(2, 2)), GraphemeId(0));
    assert_eq!(arena.intern(&unique_cluster(3, 2)), GraphemeId(0));
    assert_eq!(arena.exhausted(), 1);
}

#[test]
fn sweep_triggers_on_entries() {
    let mut arena = GraphemeArena::default();
    for n in 0..65_534 {
        arena.intern(&unique_cluster(n, 2));
    }
    assert_eq!(arena.entries(), 65_535);
    assert!(!arena.needs_sweep());
    arena.intern(&unique_cluster(65_534, 2));
    assert!(arena.needs_sweep());
}

#[test]
fn sweep_triggers_on_chars() {
    let mut arena = GraphemeArena::default();
    let mut n = 0;
    while arena.chars() < GRAPHEME_SWEEP_CHARS {
        assert!(!arena.needs_sweep());
        arena.intern(&unique_cluster(n, 16));
        n += 1;
    }
    assert!(arena.needs_sweep());
    assert!(arena.entries() < GRAPHEME_SWEEP_ENTRIES);
}

#[test]
fn sweep_keeps_live_and_remaps() {
    let mut arena = GraphemeArena::default();
    let clusters: Vec<String> = (0..100).map(|n| unique_cluster(n, 2)).collect();
    let ids: Vec<GraphemeId> = clusters.iter().map(|c| arena.intern(c)).collect();
    assert_eq!(ids[0], GraphemeId(1));
    assert_eq!(ids[99], GraphemeId(100));

    let kept: Vec<GraphemeId> = (1..=100).step_by(3).map(GraphemeId).collect();
    let live = kept
        .iter()
        .copied()
        .chain([kept[1], GraphemeId(5_000), GraphemeId(0)]);
    let remap = arena.sweep(live);

    assert_eq!(remap.kept(), 34);
    assert_eq!(arena.entries(), 35);
    for old in &kept {
        assert_eq!(arena.resolve(remap.get(*old)), clusters[old.0 as usize - 1]);
    }
    assert_eq!(remap.get(GraphemeId(2)), GraphemeId(0));
    assert_eq!(remap.get(GraphemeId(5_000)), GraphemeId(0));
    assert_eq!(remap.get(GraphemeId(0)), GraphemeId(0));
}

#[test]
fn sweep_keeps_counters() {
    let mut arena = GraphemeArena::with_limits(3, 1 << 20);
    arena.intern(&unique_cluster(0, 2));
    arena.intern(&"a".repeat(20));
    arena.intern(&unique_cluster(1, 2));
    assert_eq!((arena.truncated(), arena.exhausted()), (1, 1));
    arena.sweep([GraphemeId(1)]);
    assert_eq!((arena.truncated(), arena.exhausted()), (1, 1));
}

#[test]
fn sweep_trigger_doubles_after_a_large_live_set() {
    let mut arena = GraphemeArena::default();
    let mut ids = Vec::new();
    for n in 0..70_000 {
        ids.push(arena.intern(&unique_cluster(n, 2)));
    }
    assert!(arena.needs_sweep());
    arena.sweep(ids);
    assert_eq!(arena.entries(), 70_001);
    assert!(!arena.needs_sweep());
    for n in 70_000..140_000 {
        arena.intern(&unique_cluster(n, 2));
    }
    assert_eq!(arena.entries(), 140_001);
    assert!(!arena.needs_sweep());
    arena.intern(&unique_cluster(140_000, 2));
    assert!(arena.needs_sweep());
}

#[test]
fn hyperlink_identity() {
    let mut table = HyperlinkTable::default();
    let a = table.intern(Some("x"), "https://a").unwrap();
    assert_eq!(table.intern(Some("x"), "https://a"), Some(a));
    let b = table.intern(Some("x"), "https://b").unwrap();
    assert_ne!(a, b);

    let one = table.intern(None, "https://one").unwrap();
    assert_eq!(table.intern(None, "https://one"), Some(one));
    assert_eq!(table.intern(Some(""), "https://one"), Some(one));
    let two = table.intern(None, "https://two").unwrap();
    assert_eq!(&*table.resolve(one).unwrap().id, "1");
    assert_eq!(&*table.resolve(two).unwrap().id, "2");
    assert!(table.resolve(one).unwrap().implicit);
    assert!(!table.resolve(a).unwrap().implicit);

    let explicit_one = table.intern(Some("1"), "https://one").unwrap();
    assert_ne!(explicit_one, one);
    assert_eq!(table.resolve(HyperlinkId(999)), None);
}

#[test]
fn hyperlink_full_by_count() {
    let mut table = HyperlinkTable::default();
    for n in 0..65_536 {
        assert!(table.intern(None, &format!("u{n}")).is_some());
    }
    assert_eq!(table.len(), 65_536);
    assert_eq!(table.intern(None, "one more"), None);
    assert_eq!(table.exhausted(), 1);
    assert_eq!(table.intern(None, "u0"), Some(HyperlinkId(0)));
    assert_eq!(table.exhausted(), 1);
}

#[test]
fn hyperlink_full_by_bytes() {
    let mut table = HyperlinkTable::default();
    let uri = "u".repeat(2048);
    for n in 0..2044 {
        assert!(
            table.intern(Some(&format!("{n:04}")), &uri).is_some(),
            "{n}"
        );
    }
    assert_eq!(table.intern(Some("2044"), &uri), None);
    assert_eq!(table.exhausted(), 1);
}

#[test]
fn hyperlink_clear() {
    let mut table = HyperlinkTable::default();
    table.intern(None, "https://one");
    assert_eq!(table.intern(None, &"u".repeat(5 << 20)), None);
    table.clear();
    assert!(table.is_empty());
    let id = table.intern(None, "https://other").unwrap();
    assert_eq!(&*table.resolve(id).unwrap().id, "1");
    assert_eq!(table.exhausted(), 1);
}

#[test]
fn interner_routes_to_its_tables() {
    let mut interner = Interner::default();
    let style = Style {
        attrs: Attrs::BOLD,
        ..Style::DEFAULT
    };
    let style_id = interner.style(style);
    assert_eq!(*interner.styles.resolve(style_id.0), style);
    assert_eq!(*interner.resolve_style(style_id), style);

    let link = interner.hyperlinks.intern(None, "https://x");
    let extras = Extras {
        hyperlink: link,
        graphic: None,
    };
    let extras_id = interner.extras(extras);
    assert_eq!(*interner.extras.resolve(extras_id.0), extras);
    assert_eq!(*interner.resolve_extras(extras_id), extras);

    let grapheme = interner.grapheme("e\u{301}");
    assert_eq!(interner.graphemes.resolve(grapheme), "e\u{301}");
    assert_eq!(interner.resolve_grapheme(grapheme), "e\u{301}");
    assert!(!interner.needs_grapheme_sweep());
}
