//! Tests of the image store: keys, numbers, generated ids, eviction.

use super::*;
use crate::grid::ScreenKind::{Alternate, Primary};

/// A stored image of `pixels` pixels, every byte `fill`, with `graphic` as its pixel id.
fn stored(screen: ScreenKind, id: u32, number: u32, pixels: u32, fill: u8) -> StoredImage {
    StoredImage {
        screen,
        id,
        number,
        data: Arc::new(GraphicData {
            id: GraphicId(u64::from(id) * 100 + u64::from(fill)),
            width: pixels,
            height: 1,
            rgba: vec![fill; pixels as usize * 4],
        }),
        delivered: false,
    }
}

fn fill_of(store: &ImageStore, screen: ScreenKind, id: u32) -> Option<u8> {
    store.get(screen, id).map(|stored| stored.data.rgba[0])
}

fn ids(images: &[StoredImage]) -> Vec<u32> {
    images.iter().map(|stored| stored.id).collect()
}

#[test]
fn an_image_is_replaced_by_its_screen_and_id() {
    let mut store = ImageStore::default();
    assert!(store.insert(stored(Primary, 1, 0, 1, 10)).is_empty());
    assert!(store.insert(stored(Alternate, 1, 0, 1, 20)).is_empty());
    let gone = store.insert(stored(Primary, 1, 0, 2, 30));
    assert_eq!((ids(&gone), gone[0].data.rgba[0]), (vec![1], 10));
    assert_eq!(fill_of(&store, Primary, 1), Some(30));
    assert_eq!(fill_of(&store, Alternate, 1), Some(20));
    assert_eq!(store.bytes, 12);
    assert_eq!(store.ids(Primary).collect::<Vec<_>>(), [1]);
    assert!(store.remove(Primary, 1).is_some());
    assert!(store.remove(Primary, 1).is_none());
    assert_eq!(fill_of(&store, Alternate, 1), Some(20));
    assert_eq!(ids(&store.clear()), [1]);
    assert_eq!((store.images.len(), store.bytes), (0, 0));
}

#[test]
fn an_image_is_found_by_the_id_of_its_pixels() {
    let mut store = ImageStore::default();
    store.insert(stored(Primary, 3, 0, 1, 7));
    assert!(store.holds(GraphicId(307)));
    assert!(!store.holds(GraphicId(3)));
    store.get_mut(Primary, 3).unwrap().delivered = true;
    store.mark_undelivered(GraphicId(999));
    assert!(store.get(Primary, 3).unwrap().delivered);
    store.mark_undelivered(GraphicId(307));
    assert!(!store.get(Primary, 3).unwrap().delivered);
}

#[test]
fn a_number_names_the_newest_image_that_has_it() {
    let mut store = ImageStore::default();
    store.insert(stored(Primary, 1, 7, 1, 10));
    store.insert(stored(Primary, 2, 7, 1, 20));
    store.insert(stored(Alternate, 3, 7, 1, 30));
    store.insert(stored(Primary, 4, 0, 1, 40));
    let newest = |screen, number| {
        store
            .newest_numbered(screen, number)
            .map(|stored| stored.id)
    };
    assert_eq!(newest(Primary, 7), Some(2));
    assert_eq!(newest(Alternate, 7), Some(3));
    assert_eq!(newest(Primary, 8), None);
    assert_eq!(newest(Primary, 0), None, "0 is no number");
}

#[test]
fn generated_ids_skip_the_ids_in_use() {
    let mut store = ImageStore::default();
    store.insert(stored(Primary, GENERATED_ID_START + 1, 0, 1, 0));
    assert_eq!(store.generate_id(Primary), GENERATED_ID_START);
    assert_eq!(store.generate_id(Primary), GENERATED_ID_START + 2);
    // The other screen has its own ids, and the counter wraps inside the upper half.
    store.next_generated = u32::MAX;
    assert_eq!(store.generate_id(Alternate), u32::MAX);
    assert_eq!(store.generate_id(Alternate), GENERATED_ID_START);
}

#[test]
fn the_oldest_images_go_when_the_byte_budget_is_passed() {
    let mut store = ImageStore::with_budget(40);
    store.insert(stored(Primary, 1, 0, 4, 0));
    assert!(store.insert(stored(Primary, 2, 0, 4, 0)).is_empty());
    let gone = store.insert(stored(Alternate, 3, 0, 4, 0));
    assert_eq!(ids(&gone), [1]);
    assert!(store.get(Primary, 1).is_none());
    assert_eq!(store.bytes, 32);
    // An image larger than the whole budget still stays, alone.
    let gone = store.insert(stored(Primary, 4, 0, 100, 0));
    assert_eq!(ids(&gone), [2, 3]);
    assert_eq!((store.images.len(), store.bytes), (1, 400));
}

#[test]
fn the_oldest_image_goes_when_there_are_too_many() {
    let mut store = ImageStore::default();
    for id in 1..=STORE_MAX_IMAGES as u32 {
        assert!(store.insert(stored(Primary, id, 0, 1, 0)).is_empty());
    }
    let gone = store.insert(stored(Primary, 5000, 0, 1, 0));
    assert_eq!(ids(&gone), [1]);
    assert_eq!(store.images.len(), STORE_MAX_IMAGES);
    assert!(store.get(Primary, 2).is_some());
}
