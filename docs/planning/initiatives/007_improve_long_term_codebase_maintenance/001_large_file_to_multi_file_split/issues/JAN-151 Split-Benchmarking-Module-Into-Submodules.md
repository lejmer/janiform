## Split Benchmarking Module Into Submodules

### Description
Refactor `crates/core/src/benchmarking.rs` into `crates/core/src/benchmarking/` with `mod.rs`, preserving external imports and existing benchmark API entry points.

### Acceptance Criteria
- Create `crates/core/src/benchmarking/mod.rs` and split the large single file into cohesive submodules.
- Preserve all public contract paths from `janiform::benchmarking`.
- Keep module declaration in `lib.rs` unchanged.
- Confirm compile and benchmark-related tests/builds still pass.

### Unit tests
- Re-run benchmarking verification tests.

### Notes/Constraints
- No behavior changes, only structural refactor.
