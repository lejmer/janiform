## Split Pricing Module Into Submodules

### Description
Refactor `crates/core/src/pricing.rs` into a foldered module layout under `crates/core/src/pricing/` with `mod.rs`, preserving public API paths.

### Acceptance Criteria
- Create folder `crates/core/src/pricing/` and split file responsibilities into cohesive submodules.
- Preserve current external symbol visibility and all existing `janiform::pricing::*` paths.
- Keep root `lib.rs` import unchanged as `pub mod pricing;`.
- Add `mod.rs` re-exports so downstream callers can continue importing existing items without path changes.
- Ensure tests and compile checks pass.

### Unit tests
- Run relevant pricing test coverage after refactor.

### Notes/Constraints
- Avoid introducing duplicate logic while migrating.
- Preserve error types and behavior around existing pricing contracts.
