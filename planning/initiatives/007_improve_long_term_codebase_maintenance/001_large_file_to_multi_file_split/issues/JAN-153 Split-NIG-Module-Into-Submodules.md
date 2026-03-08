## Split NIG Module Into Submodules

### Description
Refactor `crates/core/src/nig.rs` into `crates/core/src/nig/mod.rs` with smaller domain-specific files while keeping imports stable.

### Acceptance Criteria
- Move `nig.rs` into `crates/core/src/nig/` as a module with `mod.rs` and supporting files.
- Keep public symbol paths and behavior unchanged for `janiform::nig` consumers.
- Keep `mod.rs` as just the module entrypoint.
- Preserve `pub mod nig;` declaration in `lib.rs`.
- Run NIG-related tests to confirm parity.

### Unit tests
- Re-run numerical and simulation tests relying on NIG drivers.

### Notes/Constraints
- Do not modify numerical formulas, parameter validation rules, or error surfaces during split.
