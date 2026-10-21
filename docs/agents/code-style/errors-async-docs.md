# Errors, async, and documentation

### Error handling

* No `unwrap()`, `expect()`, or `panic!()` in production code (clippy denies `unwrap_used`).
* Propagate errors with `?` and add meaningful context when doing so.
* Return domain-specific errors (`thiserror`) from libraries; `anyhow` only at the top level of a binary.
* Error messages explain what failed and why. Handle errors where enough context exists for a useful message.

### Async and concurrency

* Keep async boundaries explicit.
* Spawn background tasks only when ownership and lifetime are well understood.
* Never hold locks across `.await`.
* Share immutable data whenever possible. Keep synchronization scopes as small as practical.
* Never block a UI or event-loop thread on I/O; do the work on a runtime thread and send results back through channels.

### Documentation and comments

* Comments explain why, not what.
* Document assumptions, invariants, and ownership expectations when they are not obvious.
* Every `unsafe` block explains why it is safe and which invariants are maintained. The workspace denies `unsafe_code`; lift it per crate only with a written reason.
* Remove outdated comments when changing code.
