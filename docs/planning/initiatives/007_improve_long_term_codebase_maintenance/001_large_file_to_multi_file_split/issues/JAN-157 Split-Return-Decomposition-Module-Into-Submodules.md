## Split Return Decomposition Module Into Submodules

### Description
Refactor `crates/core/src/return_decomposition.rs` into `crates/core/src/return_decomposition/mod.rs` and split related concerns into smaller internal modules.

### Acceptance Criteria
- Replace the single `return_decomposition.rs` file with `crates/core/src/return_decomposition/` and a `mod.rs`.
- Preserve all current public `janiform::return_decomposition` import paths.
- Keep `mod.rs` as just the module entrypoint.
- Keep `pub mod return_decomposition;` unchanged in `crates/core/src/lib.rs` and avoid API surface changes.
- Ensure all existing return decomposition tests continue to compile with no caller-side import changes.

### Unit tests
- Re-run related calibration/signature tests for parity.

### Notes/Constraints
- Keep decomposition math and sign convention behavior intact while reorganizing internals.
