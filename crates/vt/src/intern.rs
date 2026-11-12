//! The tables a [`Cell`](crate::Cell) points into.
//!
//! One [`Interner`] belongs to one terminal and is shared by its screens, so an id never
//! changes meaning when the screens swap. Overflow is handled in two ways:
//!
//! - **Style, extras and hyperlink ids never move and are reclaimed by a full-grid sweep.** A
//!   full table reuses, inserts, or falls back: an issued id resolves to the same value while a
//!   cell, pen or saved cursor names it, and the terminal frees it once none does (every image
//!   takes one extras entry) so the next new value can reuse the slot.
//! - **Graphemes are collected.** [`GraphemeArena::sweep`] rebuilds the arena from the ids
//!   still in use and returns a [`GraphemeRemap`], because unique clusters are the one kind
//!   of attacker-reachable growth with no natural reuse.
//!
//! Overflow is never an error and never a panic. Every table keeps saturating counters
//! (`exhausted()`, `truncated()`) that only grow and survive `sweep` and `clear`; compare
//! them before and after a feed to learn what was dropped.
//!
//! # Limits
//!
//! | Table | Cap | At the cap |
//! |---|---|---|
//! | [`InternTable`] of styles | 1,048,576 entries including id 0 | a new value gets id 0; counted |
//! | [`InternTable`] of extras | 65,536 entries including id 0 | a new value gets id 0; counted |
//! | [`GraphemeArena`] cluster | 16 scalars | cut at 16 scalars; counted as truncated |
//! | [`GraphemeArena`] entries | 262,144 including id 0 | a new cluster gets id 0 (`" "`); counted |
//! | [`GraphemeArena`] scalars | 1,048,576 | same |
//! | [`HyperlinkTable`] links | 65,536 in use | `intern` returns `None`; counted |
//! | [`HyperlinkTable`] text | 4 MiB of id and URI bytes | same |

use std::hash::{Hash, Hasher};

use rustc_hash::{FxHashMap, FxHashSet, FxHasher};

use crate::cell::{CellContent, Style};

/// Entries in an [`InternTable`] made by `default()`, id 0 included: what a 16-bit id names.
const TABLE_CAPACITY: usize = 1 << 16;
/// Styles a terminal keeps, id 0 included: what the 20 style bits of a cell can name.
pub(crate) const STYLE_CAPACITY: usize = 1 << 20;
/// An index slot of an [`InternTable`] that holds no id.
const NO_ID: u32 = u32::MAX;
/// Index slots of a new [`InternTable`]; the index doubles to stay at most half full.
const INDEX_START: usize = 64;
/// New entries after which [`InternTable::needs_sweep`] turns true.
const SWEEP_INTERVAL: u32 = 4096;
/// Longest cluster kept, in scalars.
const GRAPHEME_MAX_LEN: usize = 16;
/// Most clusters in the arena, id 0 included. Far below the 21-bit id space a cell can
/// address, to bound memory.
pub(crate) const GRAPHEME_ID_LIMIT: u32 = 1 << 18;
/// Most scalars held by the arena.
const GRAPHEME_MAX_CHARS: usize = 1 << 20;
/// Most hyperlinks in a [`HyperlinkTable`].
const HYPERLINK_CAPACITY: usize = 1 << 16;
/// Most `id` plus `uri` bytes held by a [`HyperlinkTable`].
const HYPERLINK_MAX_BYTES: usize = 4 << 20;
/// The longest `id` plus `uri` one OSC 8 can carry: its payload cap.
const HYPERLINK_LONGEST: usize = crate::parser::OSC_INLINE;
/// New links after which a full [`HyperlinkTable`] is worth sweeping again.
const HYPERLINK_SWEEP_INTERVAL: u32 = 1024;

/// Arena entry count at which [`GraphemeArena::needs_sweep`] first turns true.
pub const GRAPHEME_SWEEP_ENTRIES: usize = 65_536;
/// Arena scalar count at which [`GraphemeArena::needs_sweep`] first turns true.
pub const GRAPHEME_SWEEP_CHARS: usize = 1 << 18;

/// Marks a `GraphemeRemap` entry whose cluster the sweep dropped; the low bits hold its first
/// scalar.
const DROPPED_BIT: u32 = 1 << 31;
/// The `GraphemeRemap` entry of an id the sweep was not told about.
const UNLISTED: u32 = 0;

/// Index of a [`Style`] in the style table. `StyleId::DEFAULT` is [`Style::DEFAULT`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct StyleId(pub u32);

impl StyleId {
    /// The default style.
    pub const DEFAULT: StyleId = StyleId(0);
}

/// Index of an [`Extras`] in the extras table. `ExtrasId::NONE` is [`Extras::NONE`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct ExtrasId(pub u16);

impl ExtrasId {
    /// No hyperlink and no image.
    pub const NONE: ExtrasId = ExtrasId(0);
}

/// Index of a cluster in the [`GraphemeArena`]. `GraphemeId(0)` is `" "`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct GraphemeId(pub u32);

/// Index of a link in the [`HyperlinkTable`], counted from 0.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct HyperlinkId(pub u32);

/// Identifier of an image placement; allocated by the graphics layer, only stored here.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct GraphicId(pub u64);

/// The rarely used attributes of a cell, kept out of the cell itself.
///
/// `graphic` says which image covers the cell, not where: one entry per image, not per cell.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Extras {
    /// The hyperlink (OSC 8) covering the cell.
    pub hyperlink: Option<HyperlinkId>,
    /// The image covering the cell.
    pub graphic: Option<GraphicId>,
}

impl Extras {
    /// No hyperlink and no image.
    pub const NONE: Extras = Extras {
        hyperlink: None,
        graphic: None,
    };
}

/// A deduplicating table of small values addressed by ids.
///
/// Id 0 always holds `T::default()`. Ids are never moved; one is reused only after its owner
/// swept it as unreferenced (the terminal does this for styles and extras). At its capacity a
/// new value gets id 0 and is counted by [`exhausted`](Self::exhausted); known values still hit.
/// The hash is not collision-resistant; the capacity bounds the worst probe chain.
#[derive(Debug)]
pub struct InternTable<T> {
    entries: Vec<T>,
    /// The hash index: ids by open addressing with linear probing, in a power of two of slots
    /// that is at most half full. An id costs 4 bytes here, not a second copy of its value,
    /// which keeps a full table of a million styles near 23 MiB.
    index: Vec<u32>,
    /// Most entries, id 0 included.
    capacity: usize,
    exhausted: u32,
    /// Swept slots; each holds `T::default()` until a new value takes it.
    free: Vec<u32>,
    /// Misses since the last sweep, the full-table ones included.
    since_sweep: u32,
    /// Misses a full table waits for before it is swept again: `SWEEP_INTERVAL`, doubled
    /// by every sweep that freed little, so a table full of values still in use is not
    /// walked over and over.
    full_interval: u32,
    /// Entries in use at which the owner sweeps although the table has room: 65,536 at
    /// first, then twice what the last sweep kept. A table with few values in use stays
    /// small enough for the cache; one with many grows towards its capacity in doublings.
    soft_limit: usize,
}

impl<T: Copy + Eq + Hash + Default> Default for InternTable<T> {
    /// A table of 65,536 entries: what a 16-bit id can name.
    fn default() -> Self {
        InternTable::with_capacity(TABLE_CAPACITY)
    }
}

impl<T: Copy + Eq + Hash + Default> InternTable<T> {
    /// A table of at most `capacity` entries, id 0 included.
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        let mut table = InternTable {
            entries: vec![T::default()],
            index: vec![NO_ID; INDEX_START],
            capacity,
            exhausted: 0,
            free: Vec::new(),
            since_sweep: 0,
            full_interval: SWEEP_INTERVAL,
            soft_limit: capacity.min(TABLE_CAPACITY),
        };
        table.index_id(0);
        table
    }

    /// Looks `value` up in the index: `Ok(id)` when it is interned, else the empty slot its id
    /// would take. The index is never full, so the walk ends.
    #[inline]
    fn probe(&self, value: &T) -> Result<u32, usize> {
        // The default value is the commonest lookup (`CSI m`) and always id 0: no hash.
        if *value == T::default() {
            return Ok(0);
        }
        let mask = self.index.len() - 1;
        let mut hasher = FxHasher::default();
        value.hash(&mut hasher);
        // Truncation is fine: only the low bits pick the slot.
        let mut slot = hasher.finish() as usize & mask;
        loop {
            match self.index.get(slot) {
                Some(&id) if id != NO_ID => {
                    if self.entries.get(id as usize) == Some(value) {
                        return Ok(id);
                    }
                }
                _ => return Err(slot),
            }
            slot = (slot + 1) & mask;
        }
    }

    /// Enters `id`, whose value is not indexed yet, into the index.
    fn index_id(&mut self, id: u32) {
        let Some(value) = self.entries.get(id as usize).copied() else {
            return;
        };
        if let Err(slot) = self.probe(&value)
            && let Some(entry) = self.index.get_mut(slot)
        {
            *entry = id;
        }
    }

    /// Rebuilds the index with `slots` slots (a power of two) from the entries in use.
    fn rebuild_index(&mut self, slots: usize) {
        self.index.clear();
        self.index.resize(slots, NO_ID);
        // Fits: `entries.len() <= capacity <= 1 << 20`.
        for id in 0..self.entries.len() as u32 {
            if !self.is_free(id) {
                self.index_id(id);
            }
        }
    }

    /// Entries in use: every id but the swept ones.
    fn in_use(&self) -> usize {
        self.entries.len() - self.free.len()
    }

    /// Returns the id of `value`, adding it if it is new and the table has room.
    pub fn intern(&mut self, value: T) -> u32 {
        let slot = match self.probe(&value) {
            Ok(id) => return id,
            Err(slot) => slot,
        };
        self.since_sweep = self.since_sweep.saturating_add(1);
        let id = if let Some(id) = self.free.pop() {
            if let Some(entry) = self.entries.get_mut(id as usize) {
                *entry = value;
            }
            id
        } else if self.entries.len() >= self.capacity {
            self.exhausted = self.exhausted.saturating_add(1);
            return 0;
        } else {
            // Fits: `len < capacity <= 1 << 20`.
            let id = self.entries.len() as u32;
            self.entries.push(value);
            id
        };
        if let Some(entry) = self.index.get_mut(slot) {
            *entry = id;
        }
        if self.in_use() * 2 > self.index.len() {
            self.rebuild_index(self.index.len() * 2);
        }
        id
    }

    /// Returns the value behind `id`; an unknown id gives the default value.
    pub fn resolve(&self, id: u32) -> &T {
        // `entries[0]` exists: every constructor pushes the default.
        self.entries.get(id as usize).unwrap_or(&self.entries[0])
    }

    /// Returns the id of `value` if it is already interned, counting nothing.
    #[inline]
    pub(crate) fn lookup(&self, value: &T) -> Option<u32> {
        self.probe(value).ok()
    }

    /// How many new values fit before the table refuses: freed slots plus the unused tail.
    pub(crate) fn room(&self) -> usize {
        self.free.len() + (self.capacity - self.entries.len())
    }

    /// Returns the number of entries, id 0 included.
    pub fn entries(&self) -> usize {
        self.entries.len()
    }

    /// Returns how many new values were refused because the table was full.
    pub fn exhausted(&self) -> u32 {
        self.exhausted
    }

    /// Back to only id 0. Ids issued before are unknown afterwards, so the caller must hold
    /// none; [`exhausted`](Self::exhausted) is kept.
    pub(crate) fn clear(&mut self) {
        self.entries.truncate(1);
        self.free.clear();
        self.since_sweep = 0;
        self.full_interval = SWEEP_INTERVAL;
        self.soft_limit = self.capacity.min(TABLE_CAPACITY);
        self.rebuild_index(INDEX_START);
    }

    /// Whether enough new entries arrived since the last sweep that one is due.
    pub(crate) fn needs_sweep(&self) -> bool {
        self.since_sweep >= SWEEP_INTERVAL
    }

    /// Whether the owner should sweep before it interns a new value (two, in fact: a pen and
    /// its erase style).
    ///
    /// With room left: once the entries in use reach the soft limit, which is 65,536 or twice
    /// what the last sweep kept. With none: once enough values were refused since the last
    /// sweep, counted against an interval that doubles while sweeps free little (up to a
    /// quarter of the capacity) and starts over when one frees a lot.
    pub(crate) fn wants_sweep(&self) -> bool {
        if self.room() < 2 {
            return self.since_sweep >= self.full_interval;
        }
        self.in_use() + 2 > self.soft_limit
    }

    /// Whether `id` is a swept slot waiting for reuse. A value equal to the default always
    /// interns to id 0, so a default at any other id marks a free slot.
    pub(crate) fn is_free(&self, id: u32) -> bool {
        id != 0 && self.entries.get(id as usize) == Some(&T::default())
    }

    /// Frees every entry (id 0 excepted) that is not in `live`: forgets its value and offers
    /// the slot to the next new value. Ids are never renumbered. The caller passes every id a
    /// cell, pen or saved cursor still holds.
    pub(crate) fn sweep_unreferenced(&mut self, live: &FxHashSet<u32>) {
        let free_before = self.free.len();
        // Fits: `entries.len() <= capacity <= 1 << 20`.
        for id in 1..self.entries.len() as u32 {
            if live.contains(&id) || self.is_free(id) {
                continue;
            }
            if let Some(slot) = self.entries.get_mut(id as usize) {
                *slot = T::default();
                self.free.push(id);
            }
        }
        let freed = self.free.len() - free_before;
        self.full_interval = if freed >= self.full_interval as usize / 2 {
            SWEEP_INTERVAL
        } else {
            // At most a quarter of the capacity: `capacity <= 1 << 20` fits `u32`.
            let ceiling = (self.capacity / 4).max(SWEEP_INTERVAL as usize) as u32;
            self.full_interval.saturating_mul(2).min(ceiling)
        };
        self.soft_limit =
            (self.in_use() * 2).clamp(TABLE_CAPACITY.min(self.capacity), self.capacity);
        // A burst of values does not pin the index at its peak, but it keeps what the soft
        // limit needs: a stream of short-lived values would otherwise regrow it, doubling by
        // doubling, after every sweep.
        let needed = (self.in_use() * 2).next_power_of_two().max(INDEX_START);
        let kept = (self.soft_limit * 2).next_power_of_two();
        self.rebuild_index(self.index.len().min(kept).max(needed));
        self.since_sweep = 0;
    }
}

/// Location of one cluster's bytes in the arena text.
#[derive(Debug, Clone, Copy)]
struct Span {
    start: u32,
    len: u8,
    /// The previous cluster with the same key hash, or `NO_CLUSTER`.
    next: u32,
}

/// The end of a hash chain.
const NO_CLUSTER: u32 = u32::MAX;

fn cluster_hash(cluster: &str) -> u64 {
    let mut hasher = FxHasher::default();
    hasher.write(cluster.as_bytes());
    hasher.finish()
}

/// Storage for grapheme clusters too long for one cell, addressed by [`GraphemeId`].
///
/// Id 0 is `" "`. Clusters are cut at 16 scalars. Past the entry or scalar cap a new cluster
/// gets id 0. Call [`needs_sweep`](Self::needs_sweep) between sequences and, when it is true,
/// [`sweep`](Self::sweep) with every id still in use.
#[derive(Debug)]
pub struct GraphemeArena {
    text: String,
    spans: Vec<Span>,
    /// Key hash to the newest cluster with that hash; older ones chain through `Span::next`.
    /// The text lives once, in `text`: no key is allocated per cluster.
    index: FxHashMap<u64, u32>,
    chars: usize,
    kept_entries: usize,
    kept_chars: usize,
    max_entries: usize,
    max_chars: usize,
    truncated: u32,
    exhausted: u32,
    /// Key buffer of [`intern_joined`](Self::intern_joined), kept for its capacity.
    scratch: String,
}

impl Default for GraphemeArena {
    fn default() -> Self {
        // `GRAPHEME_ID_LIMIT` is 1 << 18, which fits `usize` on every target.
        GraphemeArena::with_limits(GRAPHEME_ID_LIMIT as usize, GRAPHEME_MAX_CHARS)
    }
}

/// The sweep trigger: twice the live set after the last sweep, within `[base, cap]`.
/// Doubling keeps sweeps amortised O(1) when most clusters stay live: a sweep keeps at most
/// half of the cap, so the trigger always leaves room for about as many new entries as it kept.
fn sweep_trigger(base: usize, kept: usize, cap: usize) -> usize {
    base.max(kept.saturating_mul(2)).min(cap)
}

impl GraphemeArena {
    fn with_limits(max_entries: usize, max_chars: usize) -> Self {
        let mut index = FxHashMap::default();
        index.insert(cluster_hash(" "), 0);
        GraphemeArena {
            text: String::from(" "),
            spans: vec![Span {
                start: 0,
                len: 1,
                next: NO_CLUSTER,
            }],
            index,
            chars: 1,
            kept_entries: 1,
            kept_chars: 1,
            max_entries,
            max_chars,
            truncated: 0,
            exhausted: 0,
            scratch: String::new(),
        }
    }

    /// Returns the id of `cluster`, storing it if it is new.
    ///
    /// Clusters longer than 16 scalars are cut at a scalar boundary and counted by
    /// [`truncated`](Self::truncated). `""` and `" "` give id 0. When the arena is full a new
    /// cluster gets id 0 and is counted by [`exhausted`](Self::exhausted).
    pub fn intern(&mut self, cluster: &str) -> GraphemeId {
        let cluster = match cluster.char_indices().nth(GRAPHEME_MAX_LEN) {
            Some((cut, _)) => {
                self.truncated = self.truncated.saturating_add(1);
                cluster.split_at(cut).0
            }
            None => cluster,
        };
        if cluster.is_empty() {
            return GraphemeId(0);
        }
        let hash = cluster_hash(cluster);
        let head = self.index.get(&hash).copied().unwrap_or(NO_CLUSTER);
        let mut probe = head;
        while probe != NO_CLUSTER {
            if self.resolve(GraphemeId(probe)) == cluster {
                return GraphemeId(probe);
            }
            probe = self
                .spans
                .get(probe as usize)
                .map_or(NO_CLUSTER, |span| span.next);
        }
        let scalars = cluster.chars().count();
        if self.spans.len() >= self.max_entries || self.chars + scalars > self.max_chars {
            self.exhausted = self.exhausted.saturating_add(1);
            return GraphemeId(0);
        }
        // Both fit: the text holds at most `GRAPHEME_MAX_CHARS` scalars of up to 4 bytes
        // (4 MiB), and a cut cluster is at most 16 scalars (64 bytes).
        let span = Span {
            start: self.text.len() as u32,
            len: cluster.len() as u8,
            next: head,
        };
        // `spans.len() <= max_entries <= GRAPHEME_ID_LIMIT`, so the id fits 32 bits.
        let id = self.spans.len() as u32;
        self.text.push_str(cluster);
        self.spans.push(span);
        self.index.insert(hash, id);
        self.chars += scalars;
        GraphemeId(id)
    }

    /// Returns the id of the cluster `base` followed by `tail`, built in a buffer the arena
    /// keeps, so a call allocates only for a cluster that is new. Truncation at 16 scalars
    /// applies to the whole key, as for [`intern`](Self::intern).
    pub(crate) fn intern_joined(&mut self, base: CellContent, tail: &str) -> GraphemeId {
        let mut key = std::mem::take(&mut self.scratch);
        key.clear();
        match base {
            CellContent::Scalar(scalar) => key.push(scalar),
            CellContent::Grapheme(id) => key.push_str(self.resolve(id)),
        }
        key.push_str(tail);
        let id = self.intern(&key);
        self.scratch = key;
        id
    }

    /// Returns the cluster behind `id`: `" "` for id 0, `""` for an unknown id.
    pub fn resolve(&self, id: GraphemeId) -> &str {
        let Some(span) = self.spans.get(id.0 as usize) else {
            return "";
        };
        let start = span.start as usize;
        self.text
            .get(start..start + usize::from(span.len))
            .unwrap_or("")
    }

    /// Whether the arena has grown enough that a [`sweep`](Self::sweep) is due.
    ///
    /// The threshold doubles after a sweep that kept a large live set.
    pub fn needs_sweep(&self) -> bool {
        self.spans.len()
            >= sweep_trigger(GRAPHEME_SWEEP_ENTRIES, self.kept_entries, self.max_entries)
            || self.chars >= sweep_trigger(GRAPHEME_SWEEP_CHARS, self.kept_chars, self.max_chars)
    }

    /// Rebuilds the arena from the ids in `live` and returns the old-to-new id mapping.
    ///
    /// Clusters are kept in the order `live` lists them, up to half of each cap (entries and
    /// scalars): from the first cluster that would pass either half on, every further id is
    /// dropped, so a caller lists the ids it wants kept first (newest rows first). A dropped id
    /// maps to `GraphemeId(0)` in [`GraphemeRemap::get`]; the terminal keeps the first scalar
    /// of the cluster in its cells. Keeping at most half leaves room for as
    /// many new clusters as were kept before the next sweep is due.
    ///
    /// Every id still stored anywhere (both screens, saved state) must be in `live`; an id
    /// left out maps to `GraphemeId(0)`. Id 0 and unknown ids are ignored. Counters carry over.
    /// Call it only between sequences, then rewrite every stored id through the remap.
    pub fn sweep(&mut self, live: impl IntoIterator<Item = GraphemeId>) -> GraphemeRemap {
        let mut fresh = GraphemeArena::with_limits(self.max_entries, self.max_chars);
        let (keep_clusters, keep_chars) = (self.max_entries / 2, self.max_chars / 2);
        // Sized once for what the sweep can keep plus as many new ones: no rehash while it
        // rebuilds, none before the next sweep.
        let room = self.spans.len().min(keep_clusters).saturating_mul(2);
        fresh.index.reserve(room);
        fresh.spans.reserve(room);
        let mut map = vec![UNLISTED; self.spans.len()];
        let mut kept = 0;
        let mut full = false;
        for old in live {
            let Some(slot) = map.get_mut(old.0 as usize) else {
                continue;
            };
            if old.0 == 0 || *slot != UNLISTED {
                continue;
            }
            let cluster = self.resolve(old);
            let scalars = cluster.chars().count();
            full = full || kept >= keep_clusters || fresh.chars + scalars > keep_chars;
            if full {
                let first = cluster.chars().next().unwrap_or(' ');
                *slot = DROPPED_BIT | u32::from(first);
                continue;
            }
            *slot = fresh.intern(cluster).0;
            kept += 1;
        }
        fresh.truncated = self.truncated;
        fresh.exhausted = self.exhausted;
        fresh.scratch = std::mem::take(&mut self.scratch);
        fresh.kept_entries = fresh.spans.len();
        fresh.kept_chars = fresh.chars;
        *self = fresh;
        GraphemeRemap(map)
    }

    /// Returns how many clusters were cut to 16 scalars.
    pub fn truncated(&self) -> u32 {
        self.truncated
    }

    /// Returns how many new clusters were refused because the arena was full.
    pub fn exhausted(&self) -> u32 {
        self.exhausted
    }

    /// Returns the number of entries, id 0 included.
    pub fn entries(&self) -> usize {
        self.spans.len()
    }

    /// Returns the number of scalars held, id 0 included.
    pub fn chars(&self) -> usize {
        self.chars
    }
}

/// The old-to-new id mapping produced by [`GraphemeArena::sweep`].
#[derive(Debug, Default)]
pub struct GraphemeRemap(
    /// Indexed by old id: the new id, `DROPPED_BIT | first scalar` for a cluster the sweep did
    /// not keep, or `UNLISTED` (id 0) for an id `live` never named. New ids stay below 2^18 and
    /// scalars below 2^21, so neither meets the top bit.
    Vec<u32>,
);

impl GraphemeRemap {
    /// Returns the new id for `old`; an id that was not kept gives `GraphemeId(0)`.
    pub fn get(&self, old: GraphemeId) -> GraphemeId {
        match self.0.get(old.0 as usize) {
            Some(&entry) if entry & DROPPED_BIT == 0 => GraphemeId(entry),
            _ => GraphemeId(0),
        }
    }

    /// What a cell holding `old` shows after the sweep: the new cluster id, or the first scalar
    /// of a cluster the sweep dropped, so the cell keeps its base glyph. An id that `live` never
    /// named gives `GraphemeId(0)`, as [`get`](Self::get) does.
    pub(crate) fn content(&self, old: GraphemeId) -> CellContent {
        match self.0.get(old.0 as usize) {
            Some(&entry) if entry & DROPPED_BIT != 0 => {
                CellContent::Scalar(char::from_u32(entry & !DROPPED_BIT).unwrap_or(' '))
            }
            _ => CellContent::Grapheme(self.get(old)),
        }
    }

    /// Returns how many ids were kept.
    pub fn kept(&self) -> usize {
        let live = |&&entry: &&u32| entry != UNLISTED && entry & DROPPED_BIT == 0;
        self.0.iter().skip(1).filter(live).count()
    }
}

/// An OSC 8 hyperlink.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Hyperlink {
    /// The link's `id` parameter, or a generated decimal number for an implicit link.
    pub id: Box<str>,
    /// The target URI.
    pub uri: Box<str>,
    /// Whether the id was generated because the stream gave none.
    pub implicit: bool,
}

/// Deduplicating storage for OSC 8 hyperlinks.
///
/// A link is identified by its `(id, uri)` pair; a link without an id is identified by its
/// URI alone, so a program redrawing one link every frame holds one entry. At 65,536 links or
/// 4 MiB of text, [`intern`](Self::intern) returns `None` and the caller drops the link
/// attribute but still prints the text. An id never moves; it is reused only after its owner
/// swept it as unreferenced (the terminal does this when the table is about to refuse a link).
#[derive(Debug, Default)]
pub struct HyperlinkTable {
    /// `None` is a swept slot waiting in `free`.
    links: Vec<Option<Hyperlink>>,
    /// Explicit links by [`explicit_key`].
    explicit: FxHashMap<Box<str>, u32>,
    /// Reused buffer for the key of a lookup, so a known link costs no allocation.
    key: String,
    implicit: FxHashMap<Box<str>, u32>,
    next_implicit: u32,
    bytes: usize,
    exhausted: u32,
    /// Swept slots, offered to the next new link.
    free: Vec<u32>,
    /// New links since the last sweep, the refused ones included.
    since_sweep: u32,
}

impl HyperlinkTable {
    /// Returns the id of the link `(id, uri)`, adding it if it is new and there is room.
    ///
    /// `None` or `Some("")` for `id` makes an implicit link identified by `uri` alone.
    /// Returns `None` when the table is full.
    pub fn intern(&mut self, id: Option<&str>, uri: &str) -> Option<HyperlinkId> {
        let explicit_id = id.filter(|id| !id.is_empty());
        let known = match explicit_id {
            Some(id) => {
                explicit_key(&mut self.key, id, uri);
                self.explicit.get(self.key.as_str())
            }
            None => self.implicit.get(uri),
        };
        if let Some(&index) = known {
            return Some(HyperlinkId(index));
        }
        self.since_sweep = self.since_sweep.saturating_add(1);
        let generated;
        let link_id = match explicit_id {
            Some(id) => id,
            None => {
                generated = self.next_implicit.saturating_add(1).to_string();
                &generated
            }
        };
        let added = link_id.len() + uri.len();
        if self.len() >= HYPERLINK_CAPACITY || self.bytes + added > HYPERLINK_MAX_BYTES {
            self.exhausted = self.exhausted.saturating_add(1);
            return None;
        }
        let link = Hyperlink {
            id: Box::from(link_id),
            uri: Box::from(uri),
            implicit: explicit_id.is_none(),
        };
        let index = match self.free.pop() {
            Some(index) => {
                if let Some(slot) = self.links.get_mut(index as usize) {
                    *slot = Some(link);
                }
                index
            }
            None => {
                // Fits: `links.len() <= HYPERLINK_CAPACITY == 1 << 16`.
                let index = self.links.len() as u32;
                self.links.push(Some(link));
                index
            }
        };
        match explicit_id {
            Some(_) => {
                // `key` still holds this link's key from the lookup above.
                self.explicit.insert(Box::from(self.key.as_str()), index);
            }
            None => {
                self.next_implicit = self.next_implicit.saturating_add(1);
                self.implicit.insert(Box::from(uri), index);
            }
        }
        self.bytes += added;
        Some(HyperlinkId(index))
    }

    /// Returns the link behind `id`, or `None` for an unknown or swept id.
    pub fn resolve(&self, id: HyperlinkId) -> Option<&Hyperlink> {
        self.links.get(id.0 as usize)?.as_ref()
    }

    /// Returns the number of links.
    pub fn len(&self) -> usize {
        self.links.len() - self.free.len()
    }

    /// Whether the table holds no link.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns how many new links were refused because the table was full.
    pub fn exhausted(&self) -> u32 {
        self.exhausted
    }

    /// Drops every link and restarts implicit ids at `"1"`, as a full reset does. The
    /// [`exhausted`](Self::exhausted) counter is kept.
    pub fn clear(&mut self) {
        self.links.clear();
        self.explicit.clear();
        self.implicit.clear();
        self.free.clear();
        self.next_implicit = 0;
        self.bytes = 0;
        self.since_sweep = 0;
    }

    /// Whether the next new link may be refused and enough new links arrived since the last
    /// sweep that freeing the unreferenced ones is worth a walk over the grid.
    pub(crate) fn needs_sweep(&self) -> bool {
        let nearly_full = self.len() >= HYPERLINK_CAPACITY
            || self.bytes + HYPERLINK_LONGEST > HYPERLINK_MAX_BYTES;
        nearly_full && self.since_sweep >= HYPERLINK_SWEEP_INTERVAL
    }

    /// Frees every link whose id is not in `live`: forgets it and offers the slot to the next
    /// new link. Ids are never renumbered. The caller passes every id a cell, pen or saved
    /// cursor still holds, and must drop any copy of a freed id it made.
    pub(crate) fn sweep_unreferenced(&mut self, live: &FxHashSet<u32>) {
        for index in 0..self.links.len() {
            // Fits: `index < links.len() <= HYPERLINK_CAPACITY == 1 << 16`.
            let id = index as u32;
            if live.contains(&id) {
                continue;
            }
            let Some(link) = self.links.get_mut(index).and_then(Option::take) else {
                continue;
            };
            if link.implicit {
                self.implicit.remove(&*link.uri);
            } else {
                explicit_key(&mut self.key, &link.id, &link.uri);
                self.explicit.remove(self.key.as_str());
            }
            self.bytes = self.bytes.saturating_sub(link.id.len() + link.uri.len());
            self.free.push(id);
        }
        self.since_sweep = 0;
    }
}

/// The map key of an explicit link: the id's length, `:`, the id, the URI. The length prefix
/// keeps `("a", "bc")` and `("ab", "c")` apart.
fn explicit_key(key: &mut String, id: &str, uri: &str) {
    key.clear();
    key.push_str(&id.len().to_string());
    key.push(':');
    key.push_str(id);
    key.push_str(uri);
}

/// All the tables one terminal's cells point into.
///
/// The `exhausted()` and `truncated()` counters of the four tables report everything the
/// interner dropped or degraded.
#[derive(Debug)]
pub struct Interner {
    /// Styles, addressed by [`StyleId`].
    pub styles: InternTable<Style>,
    /// Extras, addressed by [`ExtrasId`].
    pub extras: InternTable<Extras>,
    /// Grapheme clusters, addressed by [`GraphemeId`].
    pub graphemes: GraphemeArena,
    /// OSC 8 hyperlinks, addressed by [`HyperlinkId`].
    pub hyperlinks: HyperlinkTable,
}

impl Default for Interner {
    fn default() -> Self {
        Interner {
            styles: InternTable::with_capacity(STYLE_CAPACITY),
            extras: InternTable::default(),
            graphemes: GraphemeArena::default(),
            hyperlinks: HyperlinkTable::default(),
        }
    }
}

impl Interner {
    /// Returns the id of `style`; a full table gives [`StyleId::DEFAULT`]. The id stays valid
    /// while a cell or pen holds it; the terminal may reuse it once none does.
    pub fn style(&mut self, style: Style) -> StyleId {
        StyleId(self.styles.intern(style))
    }

    /// Returns the style behind `id`; an unknown id gives [`Style::DEFAULT`].
    pub fn resolve_style(&self, id: StyleId) -> &Style {
        self.styles.resolve(id.0)
    }

    /// Returns the id of `extras`; a full table gives [`ExtrasId::NONE`].
    pub fn extras(&mut self, extras: Extras) -> ExtrasId {
        // The extras table holds at most 65,536 entries, so its ids fit 16 bits.
        ExtrasId(self.extras.intern(extras) as u16)
    }

    /// Returns the extras behind `id`; an unknown id gives [`Extras::NONE`].
    pub fn resolve_extras(&self, id: ExtrasId) -> &Extras {
        self.extras.resolve(u32::from(id.0))
    }

    /// Returns the id of `cluster`; see [`GraphemeArena::intern`].
    pub fn grapheme(&mut self, cluster: &str) -> GraphemeId {
        self.graphemes.intern(cluster)
    }

    /// Returns the cluster behind `id`; see [`GraphemeArena::resolve`].
    pub fn resolve_grapheme(&self, id: GraphemeId) -> &str {
        self.graphemes.resolve(id)
    }

    /// Whether a grapheme sweep is due; see [`GraphemeArena::needs_sweep`].
    pub fn needs_grapheme_sweep(&self) -> bool {
        self.graphemes.needs_sweep()
    }
}

#[cfg(test)]
#[path = "intern_tests.rs"]
mod intern_tests;
