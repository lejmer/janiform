## Tolerance Threshold Policy And Governance

### Description
Define engine-level tolerance thresholds for numerical and statistical regressions and establish governance for threshold updates.

### Acceptance Criteria
- Threshold policy covers oracle outputs, pricer outputs, and risk path summary metrics
- Policy includes interval-dependent tolerances for different `Delta` grids
- Threshold changes are versioned with rationale metadata
- Regression harnesses consume policy configuration without code changes
- Policy scope is limited to engine-level benchmark and regression workflows

### Unit tests
- Tests ensuring policy-version metadata is required
- Threshold policy parsing and schema validation
- Boundary behavior at tolerance cutoffs

### Notes/Constraints
- Keep tolerances strict enough to catch drift while avoiding flaky failures
- Centralize threshold definitions to prevent divergence across suites
- Defer model-validation acceptance thresholds across regimes to Initiative 005 Project 003
