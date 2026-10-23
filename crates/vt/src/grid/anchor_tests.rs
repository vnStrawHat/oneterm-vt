use super::*;

fn pos(row: u64, col: u16) -> Pos {
    Pos {
        row: RowId(row),
        col,
    }
}

fn mark(anchors: &mut Anchors, row: u64) -> AnchorId {
    anchors.register(AnchorKind::Mark(1), pos(row, 0))
}

#[test]
fn register_get_kind_and_iterate() {
    let mut anchors = Anchors::new();
    let a = mark(&mut anchors, 4);
    let b = anchors.register(AnchorKind::SelectionStart, pos(9, 3));
    assert_eq!(anchors.get(a), Some(pos(4, 0)));
    assert_eq!(anchors.get(b), Some(pos(9, 3)));
    assert_eq!(anchors.kind(b), Some(AnchorKind::SelectionStart));
    assert_eq!(anchors.live(), 2);
    let all: Vec<_> = anchors
        .iter()
        .map(|(id, anchor)| (id, anchor.alive))
        .collect();
    assert_eq!(all, [(a, true), (b, true)]);
    assert_eq!(anchors.live_in(RowId(0)..RowId(5)), 1);
    assert_eq!(anchors.live_in(RowId(0)..RowId(50)), 2);
}

#[test]
fn unissued_ids_read_as_none() {
    let anchors = Anchors::new();
    let mut other = Anchors::new();
    let id = mark(&mut other, 1);
    assert_eq!(anchors.get(id), None);
    assert_eq!(anchors.kind(id), None);
}

#[test]
fn released_id_is_recycled() {
    let mut anchors = Anchors::new();
    let a = mark(&mut anchors, 1);
    let b = mark(&mut anchors, 2);
    anchors.release(a);
    assert_eq!(anchors.get(a), None);
    assert_eq!(anchors.live(), 1);
    let c = mark(&mut anchors, 3);
    assert_eq!(c, a);
    assert_eq!(anchors.get(c), Some(pos(3, 0)));
    assert_ne!(c, b);
}

#[test]
fn releasing_twice_frees_the_slot_once() {
    let mut anchors = Anchors::new();
    let a = mark(&mut anchors, 1);
    anchors.release(a);
    anchors.release(a);
    let first = mark(&mut anchors, 2);
    let second = mark(&mut anchors, 3);
    assert_ne!(first, second);
}

#[test]
fn a_killed_anchor_can_still_be_released() {
    let mut anchors = Anchors::new();
    let a = mark(&mut anchors, 1);
    anchors.trim(RowId(0), RowId(5));
    assert_eq!(anchors.get(a), None);
    assert_eq!(anchors.kind(a), Some(AnchorKind::Mark(1)));
    anchors.release(a);
    assert_eq!(mark(&mut anchors, 9), a);
}

#[test]
fn set_revives_but_leaves_released_ids_alone() {
    let mut anchors = Anchors::new();
    let a = anchors.register(AnchorKind::Cursor, pos(1, 0));
    anchors.trim(RowId(0), RowId(5));
    assert_eq!(anchors.get(a), None);
    anchors.set(a, pos(7, 2));
    assert_eq!(anchors.get(a), Some(pos(7, 2)));
    anchors.release(a);
    anchors.set(a, pos(8, 0));
    assert_eq!(anchors.get(a), None);
}

#[test]
fn shift_region_moves_and_kills() {
    let mut anchors = Anchors::new();
    let outside = mark(&mut anchors, 1);
    let killed = mark(&mut anchors, 3);
    let moved = mark(&mut anchors, 5);
    let edge = mark(&mut anchors, 6);
    anchors.shift_region(RowId(3)..RowId(7), -2, RowId(3)..RowId(5));
    assert_eq!(anchors.get(outside), Some(pos(1, 0)));
    assert_eq!(anchors.get(killed), None);
    assert_eq!(anchors.get(moved), Some(pos(3, 0)));
    assert_eq!(anchors.get(edge), Some(pos(4, 0)));
    anchors.shift_region(RowId(0)..RowId(9), 3, RowId(0)..RowId(0));
    assert_eq!(anchors.get(moved), Some(pos(6, 0)));
    assert_eq!(anchors.get(killed), None, "a dead anchor stays dead");
}

#[test]
fn shift_region_saturates_and_skips_screen_owned_anchors() {
    let mut anchors = Anchors::new();
    let low = mark(&mut anchors, 1);
    let cursor = anchors.register(AnchorKind::Cursor, pos(4, 0));
    anchors.shift_region(RowId(0)..RowId(9), -5, RowId(0)..RowId(0));
    assert_eq!(anchors.get(low), Some(pos(0, 0)));
    assert_eq!(anchors.get(cursor), Some(pos(4, 0)));
    anchors.shift_region(RowId(0)..RowId(9), 0, RowId(0)..RowId(9));
    assert_eq!(anchors.get(low), None);
    assert_eq!(anchors.get(cursor), Some(pos(4, 0)));
}

#[test]
fn trim_is_lane_scoped() {
    let mut anchors = Anchors::new();
    let old = mark(&mut anchors, 3);
    let kept = mark(&mut anchors, 10);
    let alt = mark(&mut anchors, RowId::ALT_ORIGIN.0 + 2);
    anchors.trim(RowId::PRIMARY_ORIGIN, RowId(10));
    assert_eq!(anchors.get(old), None);
    assert_eq!(anchors.get(kept), Some(pos(10, 0)));
    assert!(anchors.get(alt).is_some());
    anchors.trim(RowId::ALT_ORIGIN, RowId::ALT_ORIGIN + 5);
    assert_eq!(anchors.get(alt), None);
    assert!(anchors.get(kept).is_some());
}

#[test]
fn heap_bytes_counts_both_lists() {
    let mut anchors = Anchors::new();
    assert_eq!(anchors.heap_bytes(), 0);
    let a = mark(&mut anchors, 1);
    anchors.release(a);
    assert!(anchors.heap_bytes() >= size_of::<Entry>() + size_of::<AnchorId>());
}

#[test]
fn graphic_kind_carries_its_id() {
    let mut anchors = Anchors::new();
    let id = anchors.register(AnchorKind::Graphic(GraphicId(77)), pos(2, 2));
    assert_eq!(anchors.kind(id), Some(AnchorKind::Graphic(GraphicId(77))));
    assert!(!AnchorKind::Graphic(GraphicId(77)).is_screen_owned());
    assert!(AnchorKind::ViewportTop.is_screen_owned());
}
