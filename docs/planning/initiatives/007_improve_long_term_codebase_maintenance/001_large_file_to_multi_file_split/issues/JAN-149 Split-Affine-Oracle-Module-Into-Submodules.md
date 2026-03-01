## Split Affine Oracle Module Into Submodules

### Description
Refactor `crates/core/src/affine_oracle.rs` into `crates/core/src/affine_oracle/mod.rs` with smaller files, preserving all existing imports and API shape.

### Acceptance Criteria
- Split the module into `crates/core/src/affine_oracle/` with `mod.rs` and focused children.
- Preserve all public symbol names and `janiform::affine_oracle::*` import paths.
- Keep `lib.rs` declaration unchanged (`pub mod affine_oracle;`).
- Confirm all dependent pricing/risk modules still resolve imports without edits.

### Unit tests
- Run existing affine-oracle tests and regression coverage.

### Notes/Constraints
- Do not alter solver behavior or return types as part of file reorganization.
