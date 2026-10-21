# Testing

* Add tests for every new behavior and a regression test for every bug fix.
* Keep unit tests in the same module as the code they verify (`#[cfg(test)] mod tests { use super::*; }`), so they reach private items without widening the public API.
* Do not change item visibility solely to make code testable.
* When unit tests become substantial, move them into a sibling `*_tests.rs` file:

      // resource_table.rs
      #[cfg(test)]
      mod resource_table_tests;

* Use the `tests/` directory only for integration tests that exercise the crate through its public API.
* Organize tests by feature or module. Each test verifies one behavior. Prefer deterministic tests.
* Extract reusable fixtures only after they become repetitive.
* `unwrap`/`expect` are allowed in tests (`clippy.toml`).
