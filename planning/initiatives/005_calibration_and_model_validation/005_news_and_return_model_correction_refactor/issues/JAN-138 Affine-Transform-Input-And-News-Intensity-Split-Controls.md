## Affine Transform Input And News Intensity Split Controls

### Description
Extend affine-transform and sufficient-statistics planning contracts so news-intensity handling supports explicit known and surprise controls while preserving tractable activity-coupling computation.

### Implementation details
- Refactor `crates/core/src/affine_oracle.rs` input-contract planning so known and surprise news intensities can be represented explicitly while preserving aggregate intensity usage in tractable terms where required.
- Refactor `crates/core/src/risk.rs` sufficient-statistics transform-context planning contracts so component-aware news intensity inputs are available for diagnostics and calibration wiring.
- Keep one canonical transform-equation mapping policy while clarifying which terms consume total intensity and which preserve component visibility.
- Update transform-input validation and metadata schemas for component-level controls.
- Ensure solver telemetry contracts can carry component-aware diagnostics without breaking tractability constraints.
- Align downstream planning references in verification and benchmark harnesses to updated context contracts.

### Acceptance Criteria
- Transform-input planning contracts define how known and surprise news intensities are provided and aggregated.
- Sufficient-statistics planning contracts remain consistent with the updated news-intensity model.
- Domain guards and telemetry expectations include component-aware diagnostics where applicable.
- Documentation clarifies which computations require aggregate intensity and which keep component-level visibility.

### Unit tests
- Input-validation tests for component-level intensity controls and aggregate fallbacks where defined.
- Deterministic transform-contract serialization and metadata validation.
- Consistency checks between transform-input contracts and sufficient-statistics context contracts.

### Notes/Constraints
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
- Do not bump schema versions. We don't have users yet. We can do whatever we want, no consequences. Not even an alpha version of the library has been released yet so we don't have to bump schemas during this refactor. All schemas should be called v1 even after the refactor.
- Preserve tractability-focused solver assumptions while adding explicit component semantics.
