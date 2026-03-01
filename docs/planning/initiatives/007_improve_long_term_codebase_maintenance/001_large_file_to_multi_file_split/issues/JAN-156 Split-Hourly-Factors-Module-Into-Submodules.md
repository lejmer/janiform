## Split Hourly Factors Modules Into Submodules

### Description
Refactor `crates/core/src/hourly_factors.rs` and `crates/core/src/hourly_factor_provider.rs` into a unified `crates/core/src/hourly_factors/` folder with `mod.rs`, then update import paths to use `hourly_factors` directly.

### Acceptance Criteria
- Replace top-level `hourly_factors.rs` and `hourly_factor_provider.rs` with folder structure under `crates/core/src/hourly_factors/`.
- Migrate all `hourly_factor_provider` logic into `crates/core/src/hourly_factors/` and remove the standalone `hourly_factor_provider` module.
- Update all in-repo callers to import through `janiform::hourly_factors` rather than `janiform::hourly_factor_provider`.
- Preserve behavior for indexing and interpolation semantics while reorganizing internals.
- Validate compile and relevant tests after path migration.

### Unit tests
- Re-run seasonal/hourly tests to confirm parity.

### Notes/Constraints
- Breaking import-path changes are allowed for this split.
- Do not add legacy compatibility shims.
