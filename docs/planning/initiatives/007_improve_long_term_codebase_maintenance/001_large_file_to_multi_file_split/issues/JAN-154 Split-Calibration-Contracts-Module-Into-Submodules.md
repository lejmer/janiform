## Split Calibration Contracts Module Into Submodules

### Description
Refactor `crates/core/src/calibration/contracts.rs` into `crates/core/src/calibration/contracts/` with `mod.rs`, preserving public API re-exports from `crates/core/src/calibration/mod.rs`.

### Acceptance Criteria
- Create `crates/core/src/calibration/contracts/` with `mod.rs` and split domain areas into focused files.
- Preserve all public calibration contract paths currently exposed from `janiform::calibration`.
- Keep `mod.rs` as just the module entrypoint.
- Keep `crates/core/src/calibration/mod.rs` module wiring and `pub use` behavior unchanged from the caller perspective.
- Ensure compile checks for all dependent calibration stages remain green.

### Unit tests
- Re-run affected calibration tests.

### Notes/Constraints
- No public contract changes beyond module organization.
