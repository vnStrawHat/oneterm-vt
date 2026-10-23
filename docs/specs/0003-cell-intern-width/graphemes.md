# `intern.rs`: `GraphemeArena` and its sweep

Caps and counters: see the table in [intern.md](intern.md). Clusters come from the print path
(phase 6) as `&str` slices produced by `unicode-segmentation`; this phase only stores them.

```rust
pub struct GraphemeArena { text: String, spans: Vec<Span>, index: FxHashMap<Box<str>, u32>,
    chars: usize, kept_entries: usize, kept_chars: usize, max_entries: usize, max_chars: usize,
    truncated: u32, exhausted: u32 }
pub fn intern(&mut self, cluster: &str) -> GraphemeId;
pub fn resolve(&self, id: GraphemeId) -> &str;   // id 0 -> " "; unknown -> ""
pub fn needs_sweep(&self) -> bool;
pub fn sweep(&mut self, live: impl IntoIterator<Item = GraphemeId>) -> GraphemeRemap;
pub fn truncated(&self) -> u32;  pub fn exhausted(&self) -> u32;
pub fn entries(&self) -> usize;  pub fn chars(&self) -> usize;   // incl. id 0; scalars held
```

- Storage is UTF-8 (`resolve` returns `&str`, what renderers and text extraction want).
  `Span { start: u32, len: u8 }` (bytes; 16 scalars <= 64 bytes). `Default` interns `" "` at 0.
- `intern`: cut to 16 scalars at a char boundary (count `truncated` once); `""` -> id 0;
  index hit -> id; would exceed either hard cap -> id 0 + `exhausted`; else append.
- Private `fn with_limits(entries: usize, chars: usize) -> Self`; `Default` calls it with the
  hard caps; tests call it with small ones. Derive `Debug`.
- `needs_sweep`: `entries >= trigger(GRAPHEME_SWEEP_ENTRIES, kept_entries, max_entries)` or
  the same for chars, where `trigger(base, kept, cap) = max(base, 2 * kept).min(cap)`.
  Doubling after a large live set keeps sweeps amortised O(1) instead of every `feed`.
- `sweep`: replace `self` with a fresh arena (same `max_*`, counters carried over), re-intern
  each distinct live id's text in iteration order, record `old -> new`; id 0 and unknown ids
  are not recorded. Set `kept_entries`/`kept_chars` to the new sizes. Phase 6 calls it only
  between sequences, after collecting every grapheme id in both screens and saved state.

```rust
#[derive(Debug, Default)] pub struct GraphemeRemap(FxHashMap<u32, u32>);
pub fn get(&self, old: GraphemeId) -> GraphemeId;  // not recorded -> GraphemeId(0)
pub fn kept(&self) -> usize;                      // recorded ids
```


Why the hard caps (`1 << 18` entries, `1 << 20` scalars) sit far below the 21-bit id space the
cell could address: memory. At the caps the arena holds at most 4 MiB of text; the full id
space would allow ~130 MiB from one hostile `feed` before the first sweep. Interning `" "`
hits id 0, so a blank never takes a second entry.
