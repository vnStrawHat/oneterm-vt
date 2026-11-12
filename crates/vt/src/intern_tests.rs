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
fn explicit_link_keys_keep_boundaries_apart() {
    let mut table = HyperlinkTable::default();
    let first = table.intern(Some("a"), "bc").unwrap();
    let second = table.intern(Some("ab"), "c").unwrap();
    assert_ne!(first, second);
    assert_eq!(table.intern(Some("a"), "bc"), Some(first));
    assert_eq!(table.intern(Some("ab"), "c"), Some(second));
    assert_eq!(table.len(), 2);
}

#[test]
fn a_known_explicit_link_adds_nothing() {
    let mut table = HyperlinkTable::default();
    let id = table.intern(Some("id"), "https://a").unwrap();
    for _ in 0..1000 {
        assert_eq!(table.intern(Some("id"), "https://a"), Some(id));
    }
    assert_eq!(table.len(), 1);
    // The 4 MiB budget was charged once (2 + 9 bytes): the rest fits exactly, one more does not.
    let rest = (4 << 20) - 11;
    assert_eq!(table.intern(Some("x"), &"u".repeat(rest)), None);
    assert!(table.intern(Some("x"), &"u".repeat(rest - 1)).is_some());
    assert_eq!(table.exhausted(), 1);
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

// ---- extras reclamation ----

fn image(n: u64) -> Extras {
    Extras {
        graphic: Some(GraphicId(n)),
        ..Extras::NONE
    }
}

fn live(ids: &[u16]) -> FxHashSet<u16> {
    ids.iter().copied().collect()
}

#[test]
fn freed_slot_reused_first() {
    let mut table = InternTable::<Extras>::default();
    let [a, b, c] = [1, 2, 3].map(|n| table.intern(image(n)));
    table.sweep_unreferenced(&live(&[b]));
    assert!(table.is_free(a) && table.is_free(c) && !table.is_free(b));
    // The most recently freed slot goes first, and the table does not grow while any is free.
    assert_eq!(table.intern(image(4)), c);
    assert_eq!(table.intern(image(5)), a);
    assert_eq!(table.entries(), 4);
    assert_eq!(*table.resolve(a), image(5));
    assert_eq!(table.intern(image(6)), 4);
    assert_eq!(table.entries(), 5);
}

#[test]
fn sweep_frees_exactly_unreferenced() {
    let mut table = InternTable::<Extras>::default();
    let ids: Vec<u16> = (1..=6).map(|n| table.intern(image(n))).collect();
    table.sweep_unreferenced(&live(&[ids[1], ids[4]]));
    for (index, id) in ids.iter().enumerate() {
        let kept = index == 1 || index == 4;
        assert_eq!(table.is_free(*id), !kept, "id {id}");
        assert_eq!(*table.resolve(*id) == Extras::NONE, !kept);
    }
    // Kept values still hit; swept values are forgotten and intern afresh.
    assert_eq!(table.intern(image(2)), ids[1]);
    assert_eq!(table.entries(), 7);
    let again = table.intern(image(1));
    assert!(ids.contains(&again) && !table.is_free(again));
    // A second sweep with the same live set frees nothing new and keeps the slots listed once.
    table.sweep_unreferenced(&live(&[ids[1], ids[4], again]));
    let reused: Vec<u16> = (0..3).map(|n| table.intern(image(10 + n))).collect();
    let mut sorted = reused.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 3, "no slot is handed out twice");
}

#[test]
fn every_miss_counts_towards_sweep() {
    let mut table = InternTable::<Extras>::default();
    for n in 0..4095 {
        table.intern(image(n));
    }
    assert!(!table.needs_sweep());
    for _ in 0..100 {
        table.intern(image(0));
    }
    assert!(!table.needs_sweep(), "hits do not count");
    table.intern(image(9_999));
    assert!(table.needs_sweep());
    table.sweep_unreferenced(&FxHashSet::default());
    assert!(!table.needs_sweep());

    // A full table still counts its refused values, so a sweep comes due without growth.
    let mut full = InternTable::<Extras>::default();
    for n in 0..70_000 {
        full.intern(image(n));
    }
    assert_eq!(full.entries(), 1 << 16);
    assert!(full.exhausted() > 0);
    assert!(full.needs_sweep());
    let everything: FxHashSet<u16> = (1..=u16::MAX).collect();
    full.sweep_unreferenced(&everything);
    assert!(!full.needs_sweep());
    for n in 80_000..80_000 + 4096 {
        assert_eq!(full.intern(image(n)), 0);
    }
    assert!(full.needs_sweep());
}

#[test]
fn clear_empties_free_list() {
    let mut table = InternTable::<Extras>::default();
    for n in 1..=3 {
        table.intern(image(n));
    }
    table.sweep_unreferenced(&FxHashSet::default());
    table.clear();
    assert_eq!(table.entries(), 1);
    assert!(!table.needs_sweep());
    // Not a stale slot from the old free list: the first id after a clear is 1.
    assert_eq!(table.intern(image(9)), 1);
    assert_eq!(table.entries(), 2);
}

#[test]
fn id_zero_never_freed() {
    let mut table = InternTable::<Extras>::default();
    table.intern(image(1));
    table.sweep_unreferenced(&FxHashSet::default());
    assert!(!table.is_free(0));
    assert_eq!(*table.resolve(0), Extras::NONE);
    assert_eq!(table.intern(Extras::NONE), 0);
    assert_eq!(table.entries(), 2);
}

#[test]
fn lookup_counts_nothing() {
    let mut table: InternTable<Style> = InternTable::default();
    let id = table.intern(rgb_style(1));
    let (entries, since) = (table.entries(), table.since_sweep);
    assert_eq!(table.lookup(&rgb_style(1)), Some(id));
    assert_eq!(table.lookup(&rgb_style(2)), None);
    assert_eq!(table.lookup(&Style::DEFAULT), Some(0));
    assert_eq!(table.entries(), entries);
    assert_eq!(table.exhausted(), 0);
    assert_eq!(table.since_sweep, since);
    assert!(!table.needs_sweep());
}

#[test]
fn room_counts_free_and_unused() {
    let mut table: InternTable<Style> = InternTable::default();
    assert_eq!(table.room(), 65_535);
    let ids: Vec<u16> = (1..=10).map(|n| table.intern(rgb_style(n))).collect();
    assert_eq!(table.room(), 65_525);
    let keep: FxHashSet<u16> = ids[..6].iter().copied().collect();
    table.sweep_unreferenced(&keep);
    assert_eq!(table.room(), 65_525 + 4);
}

// ---- half-cap sweep and joined interning ----

#[test]
fn sweep_keeps_half_newest_first() {
    let mut arena = GraphemeArena::with_limits(5, 1 << 20);
    let [a, b, c, d] = [0, 1, 2, 3].map(|n| unique_cluster(n, 2));
    let ids = [&a, &b, &c, &d].map(|cluster| arena.intern(cluster));
    // Newest first: d, c, b, a. Half of 5 entries is 2 clusters.
    let remap = arena.sweep([ids[3], ids[2], ids[1], ids[0]]);
    assert_eq!(remap.kept(), 2);
    assert_eq!(arena.resolve(remap.get(ids[3])), d);
    assert_eq!(arena.resolve(remap.get(ids[2])), c);
    for (old, cluster) in [(ids[1], &b), (ids[0], &a)] {
        assert_eq!(remap.get(old), GraphemeId(0));
        let first = cluster.chars().next().unwrap();
        assert_eq!(remap.content(old), CellContent::Scalar(first));
    }
    assert_eq!(
        remap.content(ids[3]),
        CellContent::Grapheme(remap.get(ids[3]))
    );
    assert_eq!(arena.entries(), 3);
}

#[test]
fn sweep_stops_keeping_after_the_first_cluster_that_does_not_fit() {
    // 2 clusters fit by count; the scalar half (8) fits one 4-scalar cluster and not a second
    // 6-scalar one, and a later short cluster is dropped too.
    let mut arena = GraphemeArena::with_limits(100, 17);
    let (long, mid, short) = (
        unique_cluster(0, 6),
        unique_cluster(1, 4),
        unique_cluster(2, 2),
    );
    let ids = [&long, &mid, &short].map(|cluster| arena.intern(cluster));
    let remap = arena.sweep([ids[1], ids[0], ids[2]]);
    assert_eq!(remap.kept(), 1);
    assert_eq!(arena.resolve(remap.get(ids[1])), mid);
    assert_eq!(remap.get(ids[2]), GraphemeId(0));
    assert!(matches!(remap.content(ids[2]), CellContent::Scalar(_)));
}

#[test]
fn sweeps_stay_amortised_when_live_exceeds_half() {
    let mut arena = GraphemeArena::with_limits(1_000, 1 << 20);
    let ids: Vec<GraphemeId> = (0..999)
        .map(|n| arena.intern(&unique_cluster(n, 2)))
        .collect();
    let remap = arena.sweep(ids);
    assert_eq!(remap.kept(), 500);
    assert!(!arena.needs_sweep());
    // The trigger is the cap: about `kept` new entries arrive before the next sweep.
    for n in 1_000..1_400 {
        arena.intern(&unique_cluster(n, 2));
    }
    assert!(!arena.needs_sweep());
    for n in 1_400..1_499 {
        arena.intern(&unique_cluster(n, 2));
    }
    assert!(arena.needs_sweep());
}

#[test]
fn an_unlisted_id_still_maps_to_id_zero() {
    let mut arena = GraphemeArena::default();
    let id = arena.intern(&unique_cluster(0, 2));
    let remap = arena.sweep([]);
    assert_eq!(remap.get(id), GraphemeId(0));
    assert_eq!(remap.content(id), CellContent::Grapheme(GraphemeId(0)));
    assert_eq!(remap.kept(), 0);
}

#[test]
fn intern_joined_equals_intern_of_the_concatenation() {
    let mut arena = GraphemeArena::default();
    let whole = arena.intern("a\u{300}\u{301}");
    assert_eq!(
        arena.intern_joined(CellContent::Scalar('a'), "\u{300}\u{301}"),
        whole
    );
    let base = arena.intern("a\u{300}");
    assert_eq!(
        arena.intern_joined(CellContent::Grapheme(base), "\u{301}"),
        whole
    );
    // Blank base: the key starts with a space, as the scalar path always did.
    assert_eq!(
        arena.intern_joined(CellContent::Scalar(' '), "\u{301}"),
        arena.intern(" \u{301}")
    );
    // Truncation at 16 scalars counts once, for the joined key.
    let before = arena.truncated();
    let tail = "\u{301}".repeat(30);
    let cut = arena.intern_joined(CellContent::Scalar('b'), &tail);
    assert_eq!(arena.truncated(), before + 1);
    assert_eq!(arena.resolve(cut).chars().count(), 16);
}

#[test]
fn sweep_sizes_the_index_once() {
    let mut arena = GraphemeArena::default();
    let ids: Vec<GraphemeId> = (0..1_000)
        .map(|n| arena.intern(&unique_cluster(n, 2)))
        .collect();
    let kept = 400;
    arena.sweep(ids.iter().copied().take(kept));
    let capacity = arena.index.capacity();
    assert!(capacity >= 2 * kept, "capacity {capacity}");
    for n in 1_000..1_000 + kept as u32 {
        arena.intern(&unique_cluster(n, 2));
    }
    assert_eq!(arena.index.capacity(), capacity);
}

#[test]
fn colliding_hashes_chain() {
    let mut arena = GraphemeArena::default();
    let x = arena.intern("x\u{301}");
    // Force `y` onto `x`'s hash bucket, as a 64-bit collision would.
    arena.index.insert(cluster_hash("y\u{301}"), x.0);
    let y = arena.intern("y\u{301}");
    assert_ne!(x, y);
    assert_eq!(arena.spans[y.0 as usize].next, x.0);
    assert_eq!(arena.intern("x\u{301}"), x);
    assert_eq!(arena.intern("y\u{301}"), y);
    assert_eq!(arena.resolve(x), "x\u{301}");
    assert_eq!(arena.resolve(y), "y\u{301}");
}

#[test]
fn many_unique_clusters_round_trip() {
    let mut arena = GraphemeArena::default();
    let clusters: Vec<String> = (0..100_000).map(|n| unique_cluster(n, 4)).collect();
    let ids: Vec<GraphemeId> = clusters.iter().map(|c| arena.intern(c)).collect();
    let distinct: FxHashSet<u32> = ids.iter().map(|id| id.0).collect();
    assert_eq!(distinct.len(), clusters.len());
    for (cluster, id) in clusters.iter().zip(&ids) {
        assert_eq!(arena.resolve(*id), cluster);
        assert_eq!(arena.intern(cluster), *id);
    }
}

#[test]
fn sweep_keeps_lookups_working() {
    let mut arena = GraphemeArena::default();
    let clusters: Vec<String> = (0..50).map(|n| unique_cluster(n, 3)).collect();
    let ids: Vec<GraphemeId> = clusters.iter().map(|c| arena.intern(c)).collect();
    let remap = arena.sweep(ids.iter().copied().skip(10));
    let entries = arena.entries();
    for (cluster, old) in clusters.iter().zip(&ids).skip(10) {
        assert_eq!(arena.intern(cluster), remap.get(*old));
    }
    assert_eq!(arena.entries(), entries);
}

#[test]
fn hyperlink_sweep_frees_and_reuses_ids() {
    let mut table = HyperlinkTable::default();
    let a = table.intern(Some("a"), "https://a").unwrap();
    let b = table.intern(None, "https://b").unwrap();
    let c = table.intern(None, "https://c").unwrap();
    table.sweep_unreferenced(&FxHashSet::from_iter([b.0]));
    assert_eq!(table.len(), 1);
    assert_eq!(table.resolve(a), None);
    assert_eq!(table.resolve(c), None);
    assert_eq!(table.resolve(b).map(|link| &*link.uri), Some("https://b"));
    // A swept link is new again and takes a freed slot; a kept one is still known.
    let again = table.intern(Some("a"), "https://a").unwrap();
    assert!(again == a || again == c);
    assert_eq!(table.resolve(again).map(|link| &*link.id), Some("a"));
    assert_eq!(table.intern(None, "https://b"), Some(b));
    assert_eq!(table.len(), 2);
    assert_eq!(table.exhausted(), 0);
}

#[test]
fn hyperlink_sweep_gives_the_bytes_back() {
    let mut table = HyperlinkTable::default();
    let uri = "u".repeat(2048);
    assert!(!table.needs_sweep());
    for n in 0..2044 {
        assert!(table.intern(Some(&format!("{n:04}")), &uri).is_some());
    }
    assert!(table.needs_sweep());
    table.sweep_unreferenced(&FxHashSet::default());
    assert!(table.is_empty());
    // Nothing new arrived since the sweep, so another one is not due.
    assert!(!table.needs_sweep());
    assert!(table.intern(Some("2044"), &uri).is_some());
    assert_eq!(table.exhausted(), 0);
}
