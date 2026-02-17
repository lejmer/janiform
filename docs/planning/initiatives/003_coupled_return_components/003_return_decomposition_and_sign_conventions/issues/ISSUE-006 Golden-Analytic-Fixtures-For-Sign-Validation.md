## ISSUE-006: Golden Analytic Fixtures For Sign Validation

### Description
Create focused golden fixtures derived from simplified analytic cases to validate decomposition and sign conventions with high confidence.

### Acceptance Criteria
- Fixture set includes analytically tractable scenarios for buy-only, sell-only, and combined cases
- Fixtures are reusable by both deterministic and integration test suites
- Output schema and metadata are machine-readable and documented

### Unit tests
- Golden tests comparing computed outputs against analytic fixture expectations and tests ensuring fixture metadata includes version and scenario intent
- Regression tests detecting drift from known-good sign behavior

### Notes/Constraints
- Keep fixtures compact and interpretable
- Separate deterministic analytic fixtures from stochastic tolerance tests
