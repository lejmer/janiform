## ISSUE-007: API Contract Test Matrix And Regression Suite

### Description
Build a contract-focused test matrix that validates documented behavior across supported workflows and catches regressions in signatures, shapes, and semantics.

### Acceptance Criteria
- Test matrix covers nominal, validation-failure, and edge-case scenarios.
- Regression suite verifies response schema consistency across runs.
- Contract tests cover both direct API calls and boundary integration points.
- Calibration contract tests include default `execution_mode=memory`, explicit override to `disk`, and deterministic mode-validation failures.
- Failure reports include contract ID and scenario context.

### Unit tests
- Contract tests for simulation, calibration, and pricing responses and field presence and type consistency.
- Negative tests for invalid request payloads and error contract behavior.
- Mode-default and mode-override tests for calibration API contracts.

### Notes/Constraints
- Keep matrix maintainable and focused on public behavior.
- Separate contract checks from deep algorithmic validation tests.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
