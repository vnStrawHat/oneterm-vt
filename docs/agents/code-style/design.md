# Design, ownership, and performance

### Types and design

* Prefer domain-specific types over primitive values whenever practical.
* Prefer structs over tuples for structured data.
* Prefer enums over boolean parameters (`SaveMode::Force` instead of `save(force: bool)`).
* Prefer composition over unnecessary abstraction. Avoid traits until multiple implementations or generic behavior are required.
* Constructors return fully initialized, valid objects. Avoid partially initialized state.
* Group related parameters into configuration structs when signatures become hard to read.
* Small duplication is preferable to premature abstraction.

### Functions

* Keep functions focused on one responsibility. Extract helpers when logical steps have meaningful names.
* Prefer early returns over deeply nested control flow.
* Prefer pattern matching for enums and state machines.
* Avoid long parameter lists; introduce a domain type when appropriate.
* Prefer explicit control flow over clever one-liners.

### Ownership and borrowing

* Prefer borrowing over cloning. Clone only when ownership requires it or it significantly simplifies the code.
* Keep mutable borrows as short as possible. Prefer immutable data; keep mutable state localized.
* Avoid unnecessary shared ownership with `Rc` or `Arc`.
* Pass dependencies explicitly rather than relying on global state.

### Collections

* Choose collection types intentionally based on access patterns.
* Prefer iterator adapters when they improve readability, explicit loops when chains become hard to follow.
* Avoid allocating intermediate collections unless necessary.
* Prefer `HashMap`, `BTreeMap`, `Vec` unless a specialized collection provides clear value.

### State management

* Minimize mutable state. Keep state ownership obvious. No global mutable state.
* Prefer explicit state transitions over implicit side effects.
* Make invalid states difficult to represent.

### Performance

* Measure before optimizing. Prefer readable code over micro-optimizations.
* Optimize algorithms before syntax. Avoid unnecessary allocations and cloning in hot paths.
