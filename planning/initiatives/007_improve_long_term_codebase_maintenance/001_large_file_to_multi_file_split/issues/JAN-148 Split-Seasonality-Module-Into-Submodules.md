## Split Seasonality Module Into Submodules

### Description
Refactor `crates/janiform/src/seasonality.rs` into `crates/janiform/src/seasonality/mod.rs` and split internal responsibilities by domain while keeping public usage unchanged.

### Acceptance Criteria
- Create `crates/janiform/src/seasonality/` and decompose the large file into cohesive submodules.
- Preserve existing public imports and `janiform::seasonality` API for downstream users.
- Keep `mod.rs` as just the module entrypoint.
- Keep the `pub mod seasonality;` declaration in `lib.rs` unchanged.
- Confirm all seasonality-related compile paths are stable.

### Unit tests
- Run existing seasonality tests after refactor.

### Notes/Constraints
- Keep shared helper functions accessible where currently required by other modules.
