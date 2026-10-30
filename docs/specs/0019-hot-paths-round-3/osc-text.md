# S2: OSC text without re-validation or allocation (terminal, intern, events)

## Root cause (profiles after S1)

| Fixture | Cost | Where |
|---|---|---|
| `hyperlinks` | `Utf8Chunks::next` 19 %, system heap 22 %, `memcpy` 9 % | `join_lossy` decodes each URI with `String::from_utf8_lossy` (no ASCII fast path); `HyperlinkTable::intern` boxes `id` and `uri` for every lookup of an explicit link (the `ponytail:` comment there asks for exactly this fix "if profiling ever shows OSC 8 on a hot path") |
| `osc_9_7` | `Utf8Chunks::next` 11 %, `join_lossy` 7 %, `from_utf8_lossy` 5 % | `join_lossy` again, then `EventBatch::text_span` validates the same `&str` a second time through `finish_lossy` |

## Design

| Item | Change |
|---|---|
| `Handler::join_lossy` (`terminal/osc.rs`) | per parameter: `str::from_utf8` first (word-at-a-time ASCII path); `String::from_utf8_lossy` only when it fails. Same text |
| `HyperlinkTable` (`intern.rs`) | `explicit: FxHashMap<Box<str>, u32>` keyed by `explicit_key`; new private field `key: String`, reused for every lookup |
| `explicit_key(key, id, uri)` (new, private fn) | `key` = decimal `id.len()`, `:`, `id`, `uri`; the length prefix keeps `("a", "bc")` and `("ab", "c")` apart |
| `HyperlinkTable::intern` | lookup with `explicit.get(key.as_str())` (borrowed, no allocation); on insert `Box::from(key.as_str())`; `clear` also clears the map as today. Implicit links unchanged (already borrowed) |
| `EventBatch::text_span` (`events/batch.rs`) | `append` keeps all of `text` or nothing, so the stored bytes are the `&str`: return the span `append` gives (or `(mark, 0)` when it refuses), no `finish_lossy` |
| `EventBatch::finish_lossy` | delete: `text_span` was its only caller (dead code otherwise; its unit test goes too) |

```rust
pub(crate) fn text_span(&mut self, text: &str) -> StrSpan {
    let mark = u32::try_from(self.mark()).unwrap_or(u32::MAX);
    let (start, len) = self.append(text.as_bytes()).unwrap_or((mark, 0));
    StrSpan { start, len }
}
```

- Ids, link text, event text and spans are unchanged for every input (a refused `append`
  gave an empty span at `mark` before too). `Hyperlink` and the public `intern` signature
  are unchanged; `key` is private.
- Memory per explicit link: one boxed key of `len(id) + len(uri) + 1..6` bytes instead of two
  boxes; the 4 MiB text budget counts `id + uri` as before.

## Measured (pinned MiB/s, separate builds on top of S1)

| Change | `hyperlinks` | `osc_9_7` |
|---|---:|---:|
| S1 | 923 | 302 |
| + borrowed link key only | 1,000 | 304 |
| + strict-first `join_lossy` | 1,257 | 311 |
| + `text_span` without re-validation | 1,117-1,257 | 336 |

(`hyperlinks` spreads 10 % between runs at this speed; evidence.md has the step table.)

## Not done

- Building the OSC 9 body straight into the event arena (skips the `scratch` copy): a few
  per cent of `osc_9_7` at most, not measured as a step.
