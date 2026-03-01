## Split Seasonality Module Into Submodules

### Description
Refactor `crates/core/src/seasonality.rs` into `crates/core/src/seasonality/mod.rs` and split internal responsibilities by domain while keeping public usage unchanged.

### Acceptance Criteria
- Create `crates/core/src/seasonality/` and decompose the large file into cohesive submodules.
- Preserve existing public imports and `janiform::seasonality` API for downstream users.
- Keep the `pub mod seasonality;` declaration in `lib.rs` unchanged.
- Confirm all seasonality-related compile paths are stable.

### Unit tests
- Run existing seasonality tests after refactor.

### Notes/Constraints
- Keep shared helper functions accessible where currently required by other modules.
