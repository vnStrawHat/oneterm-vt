# Structure and organization

### General principles

* Prioritize correctness, readability, and maintainability over cleverness or premature optimization.
* Write idiomatic Rust. Prefer standard library types and patterns whenever practical.
* Follow the existing architecture and coding style of the crate. Consistency is more valuable than personal preference.
* Prefer extending existing code over introducing new abstractions.
* Keep changes localized. Avoid unrelated refactoring while implementing a feature or fixing a bug.
* Optimize only after profiling identifies a real bottleneck.
* `#[cfg(windows)]` goes on the FFI call, not on the decision around it. A helper used only from platform-gated code is gated `#[cfg(any(windows, test))]` so its tests still run everywhere — never `#[allow(dead_code)]`.
* Do not preserve backward compatibility. Remove obsolete paths instead of adding compatibility layers, fallbacks, or migrations.

### Crate organization

* A crate represents a single domain or capability (`cluster`, `scheduler`, `terminal`), not a layer (`models`, `services`, `helpers`).
* Prefer extending an existing crate before creating a new one.
* Dependencies flow in one direction. If two crates depend on each other, extract the shared part into a new crate.
* Extract reusable functionality into a dedicated crate only after multiple crates need it.

### Module organization

* Organize modules around domain concepts (`resource_table.rs`, `resource_table_columns.rs`), not implementation details (`columns.rs`, `helpers.rs`).
* A module represents one primary concept. Prefer extending an existing module before creating a new one.
* Avoid folders that contain only a single source file.
* Do not use `mod.rs` unless the module naturally contains multiple related files.
* Keep related types, implementations, helper functions, and tests close together.

### File organization

* A file has one primary responsibility. Split by concept (`workload.rs`, `workload_logs.rs`), never by size (`workload_part1.rs`).
* Keep `lib.rs` and `mod.rs` focused on module declarations and public exports.
* Prefer local helper functions over `helper.rs`, `common.rs`, or `utils.rs`. If helper code becomes reusable, extract a module with a descriptive name.

### Public APIs

* Keep public APIs intentionally small. Private by default; `pub(crate)` over `pub` when wider visibility is unnecessary.
* Design APIs that are difficult to misuse. Expose behavior, not internal implementation details.
