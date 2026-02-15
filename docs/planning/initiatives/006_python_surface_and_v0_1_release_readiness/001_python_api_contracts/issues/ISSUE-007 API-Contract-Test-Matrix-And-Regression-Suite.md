## ISSUE-007: API Contract Test Matrix And Regression Suite

### Description
Build a contract-focused test matrix that validates stable behavior across supported workflows and catches regressions in signatures, shapes, and semantics.

### Acceptance Criteria
- Test matrix covers nominal, validation-failure, and edge-case scenarios.
- Regression suite verifies response schema consistency across runs.
- Contract tests cover both direct API calls and boundary integration points.
- Failure reports include contract ID and scenario context.

### Unit tests
- Contract tests for simulation, calibration, and pricing responses.
- Regression tests for field presence and type stability.
- Negative tests for invalid request payloads and error contract behavior.

### Notes/Constraints
- Keep matrix maintainable and focused on public behavior.
- Separate contract checks from deep algorithmic validation tests.
