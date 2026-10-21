# Format and naming

## Style & format

- Run `cargo fmt` before committing. Config lives in `.rustfmt.toml`.
- `cargo clippy --workspace --all-targets -- -D warnings` **must pass** before merging.
- Naming: `snake_case` (function, variable, module), `PascalCase` (type, trait, enum variant), `SCREAMING_SNAKE_CASE` (const).
- **English-only** — all code comments, doc comments (`///`), and any written content in the codebase **must be in English**. This is a hard rule with zero exceptions.
- Group imports, separated by a blank line: std, external crates, this crate.

```rust
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::state::AppState;
```

## Naming

* Name files, modules, types, and functions after business concepts.
* Prefer descriptive names (`Invoice`, `Scheduler`, `Kubeconfig`) over generic ones (`Manager`, `Processor`, `Helper`, `Common`, `Util`).
* Avoid abbreviations unless they are widely understood.
* Name functions after what they do, not how they do it.
* Name boolean variables so they read naturally in conditions.
