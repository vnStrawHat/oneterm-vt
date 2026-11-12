//! The images a program transmitted with the kitty graphics protocol and may place again.
//!
//! An image is kept under the id the program chose, per screen, until it is replaced, deleted
//! or evicted. Its pixels are shared with the embedder (one `Arc<GraphicData>`), so placing it
//! copies nothing, and forgetting it here leaves the placements that show it alone.

use std::sync::Arc;

use super::GraphicData;
use crate::grid::ScreenKind;
use crate::intern::GraphicId;

/// Most pixel bytes the store keeps, both screens together.
pub(crate) const STORE_MAX_BYTES: usize = 128 << 20;
/// Most images the store keeps: a lookup walks them all.
pub(crate) const STORE_MAX_IMAGES: usize = 1024;
/// The first id the engine gives an image that came with a number (`I`) and no id.
const GENERATED_ID_START: u32 = 1 << 31;

/// One transmitted image.
#[derive(Debug)]
pub(crate) struct StoredImage {
    /// The screen that was active when the image arrived.
    pub(crate) screen: ScreenKind,
    /// The image id of the protocol, never 0.
    pub(crate) id: u32,
    /// The image number, 0 when the program gave none.
    pub(crate) number: u32,
    /// The pixels, under the `GraphicId` the embedder knows them by.
    pub(crate) data: Arc<GraphicData>,
    /// Whether the pixels were queued for the embedder; they are, with the first placement.
    pub(crate) delivered: bool,
}

/// The stored images of both screens, oldest first.
#[derive(Debug)]
pub(crate) struct ImageStore {
    images: Vec<StoredImage>,
    bytes: usize,
    next_generated: u32,
    max_bytes: usize,
}

impl Default for ImageStore {
    fn default() -> Self {
        ImageStore::with_budget(STORE_MAX_BYTES)
    }
}

impl ImageStore {
    pub(crate) fn with_budget(max_bytes: usize) -> ImageStore {
        ImageStore {
            images: Vec::new(),
            bytes: 0,
            next_generated: GENERATED_ID_START,
            max_bytes,
        }
    }

    /// The image `id` of `screen`.
    pub(crate) fn get(&self, screen: ScreenKind, id: u32) -> Option<&StoredImage> {
        self.images
            .iter()
            .find(|stored| stored.screen == screen && stored.id == id)
    }

    /// The image `id` of `screen`, to mark it delivered.
    pub(crate) fn get_mut(&mut self, screen: ScreenKind, id: u32) -> Option<&mut StoredImage> {
        self.images
            .iter_mut()
            .find(|stored| stored.screen == screen && stored.id == id)
    }

    /// The newest image of `screen` that came with `number`; 0 is no number.
    pub(crate) fn newest_numbered(&self, screen: ScreenKind, number: u32) -> Option<&StoredImage> {
        self.images
            .iter()
            .rev()
            .find(|stored| number != 0 && stored.screen == screen && stored.number == number)
    }

    /// The ids of `screen`'s images, oldest first.
    pub(crate) fn ids(&self, screen: ScreenKind) -> impl Iterator<Item = u32> + '_ {
        self.images
            .iter()
            .filter(move |stored| stored.screen == screen)
            .map(|stored| stored.id)
    }

    /// Whether an image of either screen has the pixels `graphic`.
    pub(crate) fn holds(&self, graphic: GraphicId) -> bool {
        self.images.iter().any(|stored| stored.data.id == graphic)
    }

    /// The pixels `graphic` never reached the embedder: the next placement queues them again.
    pub(crate) fn mark_undelivered(&mut self, graphic: GraphicId) {
        if let Some(stored) = self
            .images
            .iter_mut()
            .find(|stored| stored.data.id == graphic)
        {
            stored.delivered = false;
        }
    }

    /// An id no image of `screen` has, from the upper half of the id space.
    pub(crate) fn generate_id(&mut self, screen: ScreenKind) -> u32 {
        // Ends: at most `STORE_MAX_IMAGES` ids are taken.
        loop {
            let id = self.next_generated;
            self.next_generated = id.checked_add(1).unwrap_or(GENERATED_ID_START);
            if self.get(screen, id).is_none() {
                return id;
            }
        }
    }

    /// Stores `image` as the newest one and returns what it pushed out: the image with the
    /// same screen and id, then the oldest images until both ceilings hold. The new image
    /// itself always stays.
    pub(crate) fn insert(&mut self, image: StoredImage) -> Vec<StoredImage> {
        let mut gone: Vec<StoredImage> = self.remove(image.screen, image.id).into_iter().collect();
        self.bytes += image.data.rgba.len();
        self.images.push(image);
        while self.images.len() > 1
            && (self.bytes > self.max_bytes || self.images.len() > STORE_MAX_IMAGES)
        {
            let oldest = self.images.remove(0);
            self.bytes -= oldest.data.rgba.len();
            gone.push(oldest);
        }
        gone
    }

    /// Forgets the image `id` of `screen` and returns it.
    pub(crate) fn remove(&mut self, screen: ScreenKind, id: u32) -> Option<StoredImage> {
        let index = self
            .images
            .iter()
            .position(|stored| stored.screen == screen && stored.id == id)?;
        let removed = self.images.remove(index);
        self.bytes -= removed.data.rgba.len();
        Some(removed)
    }

    /// Forgets every image and returns them; generated ids go on from where they were.
    pub(crate) fn clear(&mut self) -> Vec<StoredImage> {
        self.bytes = 0;
        std::mem::take(&mut self.images)
    }
}

#[cfg(test)]
#[path = "image_store_tests.rs"]
mod image_store_tests;
