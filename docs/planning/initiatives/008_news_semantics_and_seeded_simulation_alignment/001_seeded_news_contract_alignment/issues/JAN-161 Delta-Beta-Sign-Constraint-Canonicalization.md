## Delta-Beta Sign Constraint Canonicalization

### Description
Enforce the model-required `delta_beta < 0` skew-shift constraint consistently across core model construction and Python validation surfaces, not only in calibration-stage logic.

### Acceptance Criteria
- Add or update canonical validation logic so `nig_shared_plus_shift_configuration.delta_beta` must be finite and strictly negative in all relevant model-construction paths.
- Ensure pricing and risk request validation paths reject non-negative `delta_beta` with deterministic, actionable errors.
- Ensure the constraint behavior is consistent between Rust core validation and Python wrapper validation.

### Unit tests
- Rust test case that verifies non-negative `delta_beta` is rejected at the core NIG shared-plus-shift construction boundary.
- Python contract tests for pricing and risk payload validation that verify non-negative `delta_beta` is rejected with the expected field path.
- Regression test ensuring negative `delta_beta` payloads that satisfy all other domain constraints remain accepted.

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Keep schema identifiers and contract version tags at `v1` unless a separate versioning task explicitly requires a change.
