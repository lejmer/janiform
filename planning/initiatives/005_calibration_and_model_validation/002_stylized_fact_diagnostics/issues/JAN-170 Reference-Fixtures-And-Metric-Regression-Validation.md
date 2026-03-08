## Reference Fixtures And Metric Regression Validation

### Description
Create reference fixtures and regression validation suite to lock down metric correctness and guard against diagnostics drift.

### Acceptance Criteria
- Fixture set covers core stylized-fact categories across at least two horizons
- Regression suite enforces expected metric outputs within versioned tolerances
- Fixture provenance and update workflow are documented
- Regression outputs include actionable mismatch diagnostics
- Fixture catalog includes sparse-sample regimes that must produce `insufficient_data`
- Regression policy distinguishes deterministic exact-match fixtures from stochastic confidence-interval fixtures

### Unit tests
- Fixture schema validation and metadata completeness
- Tolerance-policy application and boundary behavior
- Metric regression tests against reference fixtures
- Bootstrap confidence-interval regression checks under fixed seed and block configuration

### Notes/Constraints
- Keep fixture set small enough for fast repeatable execution
- Separate deterministic fixture checks from stochastic benchmarking runs
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
