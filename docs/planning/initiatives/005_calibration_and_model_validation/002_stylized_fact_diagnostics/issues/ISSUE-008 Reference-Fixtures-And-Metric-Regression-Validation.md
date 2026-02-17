## ISSUE-008: Reference Fixtures And Metric Regression Validation

### Description
Create reference fixtures and regression validation suite to lock down metric correctness and guard against diagnostics drift.

### Acceptance Criteria
- Fixture set covers core stylized-fact categories across at least two horizons
- Regression suite enforces expected metric outputs within versioned tolerances
- Fixture provenance and update workflow are documented
- Regression outputs include actionable mismatch diagnostics

### Unit tests
- Fixture schema validation and metadata completeness
- Tolerance-policy application and boundary behavior
- Metric regression tests against reference fixtures

### Notes/Constraints
- Keep fixture set small enough for fast repeatable execution
- Separate deterministic fixture checks from stochastic benchmarking runs
