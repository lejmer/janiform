## Split Hawkes Module Into Submodules

### Description
Refactor `crates/janiform/src/hawkes.rs` into `crates/janiform/src/hawkes/` and a `mod.rs`, preserving public interfaces and import behavior.

### Acceptance Criteria
- Move `crates/janiform/src/hawkes.rs` into a folder module with clear submodule boundaries.
- Preserve existing external imports from `janiform::hawkes` and nested public symbols.
- Avoid changes to import paths for any public or re-exported Hawkes types/functions.
- Keep `lib.rs` module declaration unchanged.
- Keep `mod.rs` as just the module entrypoint.
- Validate core compile + test pass.

### Unit tests
- Ensure Hawkes-focused regression tests continue to pass.

### Notes/Constraints
- Keep `unsafe` usage and invariants unchanged as part of code movement.
