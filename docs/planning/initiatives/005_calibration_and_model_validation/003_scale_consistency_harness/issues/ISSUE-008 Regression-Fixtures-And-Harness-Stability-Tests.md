## ISSUE-008: Regression Fixtures And Harness Stability Tests

### Description
Create regression fixtures and stability tests that lock down harness behavior and protect against unintended changes in consistency outcomes.

### Acceptance Criteria
- Fixture set covers representative interval and parameter-regime combinations
- Regression suite validates pass-or-fail outcomes against versioned expectations
- Harness output remains stable across repeated runs on fixed fixtures
- Fixture-update workflow is documented

### Unit tests
- Scenario outcomes on reference fixtures
- Fixture-schema validation and metadata completeness
- Determinism tests for repeated harness executions

### Notes/Constraints
- Keep fixture suite compact enough for fast regression runs
- Separate deterministic fixture checks from large stochastic benchmark jobs
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
