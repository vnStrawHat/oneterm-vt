# S2 erase style, S3 sweep sizing, S4 key-less cluster index

## S2 root cause: two style lookups per SGR (`dense_cells`)

Profile: `HashMap::get` 32 % of `dense_cells`, from two lookups per `CSI 48;5;n m`:
`intern_pen_style` (the pen) and `Screen::set_template` (the erase cell, a style holding only
the pen's background). Each hashes the 14-byte `Style` field by field (11 dependent FxHash
steps). In `dense_cells`, as in any SGR that sets only a background, the pen style *is* the
bg-only style.

## S2 design

`Screen::set_template`, when the background changed and is not the default:

```rust
let bg_only = Style { bg, ..Style::DEFAULT };
let erase_style = if bg == Style::DEFAULT.bg {
    StyleId::DEFAULT
} else if *interner.resolve_style(template.style_id()) == bg_only {
    template.style_id()          // the pen is its own erase style: no second lookup
} else {
    interner.style(bg_only)
};
```

Same ids as today (the table interns one id per distinct style, so the lookup would have
returned exactly `template.style_id()`); no behaviour change.

Rejected after measuring: a two-word `Hash` for `Style` (packed colours) cost `dense_cells`
10 % (241 -> 218) and `heavy_sgr` 3 %; the derived hash stays.

## S3 root cause: the sweep rebuilds its index by doubling (`combining_unique`)

Profile: `RawTable::reserve_rehash` 19.5 %. `GraphemeArena::sweep` builds a fresh arena whose
index starts empty and doubles up to ~131,000 entries on every sweep (one per ~131,000 new
clusters), then reserves once more at the end.

## S3 design

In `sweep`, before the rebuild: `room = min(spans.len(), max_entries / 2) * 2`;
`fresh.index.reserve(room)` and `fresh.spans.reserve(room)`; drop the trailing
`fresh.index.reserve(fresh.index.len())`. One allocation per sweep, sized for what it can keep
plus as many new clusters before the next sweep (the trigger is twice the kept count).

## S4 root cause: one boxed key per unique cluster (`combining_unique`)

At HEAD, samples in the system heap (alloc and free outside the image) are 35 % of
`combining_unique`, plus `HashMap::insert`/`get` 21 %: `index: FxHashMap<Box<str>, u32>` boxes
every new cluster's text a second time (it already lives in `text`), and a sweep frees them
all. 0017 deferred this as "needs a raw-entry table"; std has none, but a hash chain does
the same with std types.

## S4 design (std only, no new dependency)

| Item | Change |
|---|---|
| `GraphemeArena::index` | `FxHashMap<u64, u32>`: FxHash of the cluster bytes -> the newest id with that hash |
| `Span` (private) | gains `next: u32`: the previous id with the same hash, or `NO_CLUSTER` (`u32::MAX`) |
| `cluster_hash(&str) -> u64` (private) | `FxHasher::write(bytes)` then `finish` |
| `intern` lookup | walk the chain from `index[hash]`, compare `resolve(id) == cluster`; first match wins |
| `intern` insert | new span with `next = head`, then `index.insert(hash, id)` |
| `with_limits` | id 0 (`" "`) entered the same way |
| `sweep` | unchanged logic; it re-interns into the fresh arena, so chains rebuild |

- Ids, truncation, caps, counters, `resolve`, `GraphemeRemap`: unchanged. Memory per entry:
  16 bytes in the map, no heap block (was a 16-byte map key plus a boxed copy of the text).
- A hash collision costs one extra text compare; correctness never depends on the hash.

## Expected (pinned MiB/s)

| Fixture | before (S1) | S2 | S3 | S4 |
|---|---:|---:|---:|---:|
| `dense_cells` | 190 | 216 | - | - |
| `combining_unique` | 18.7 | - | 22.0 | 48.7 |
| `grapheme_clusters` | 59.4 | - | 59.8 | 58.2 (within noise) |

Measured on top of S5 instead, S2 gave `dense_cells` 204 -> 241 (+18 %).
