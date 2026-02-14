## ISSUE-008: Numeric Tolerance Policy And Method Comparison

### Description
Define numeric tolerance policy and comparison checks across supported integration methods to ensure solver outputs remain reliable and interpretable.

### Acceptance Criteria
- Tolerance policy is documented for key outputs used by pricing and risk consumers.
- Method comparison harness exists for default and alternative integrator settings.
- Policy defines pass or fail criteria for output agreement checks.
- Comparison results are emitted in machine-readable format.
- Policy includes guidance for interval-dependent tolerances when `Delta` changes across grids.

### Unit tests
- Tests for tolerance-policy parsing and validation.
- Method-comparison tests on controlled fixtures with expected agreement behavior.
- Regression tests ensuring stable comparison outputs over time.

### Notes/Constraints
- Keep tolerance settings configurable but versioned.
- Do not treat method-comparison checks as a replacement for reference fixtures.
