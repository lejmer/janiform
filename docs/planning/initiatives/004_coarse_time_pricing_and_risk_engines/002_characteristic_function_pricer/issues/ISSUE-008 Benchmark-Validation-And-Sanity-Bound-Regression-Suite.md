## ISSUE-008: Benchmark Validation And Sanity Bound Regression Suite

### Description
Create benchmark and regression validation suite for pricer outputs against reference scenarios and sanity bounds.

### Acceptance Criteria
- Reference benchmark scenarios exist for baseline vanilla option configurations
- Regression suite checks output stability against expected tolerances
- Sanity-bound checks flag implausible prices and integration pathologies
- Benchmark artifacts are versioned and documented

### Unit tests
- Benchmark-result schema tests for artifact stability
- Sanity-bound tests for monotonicity and no-arbitrage style guardrails where applicable
- Regression tests against reference pricing fixtures

### Notes/Constraints
- Keep benchmark workloads bounded for repeatable CI execution
- Separate strict fixture checks from tolerance-based benchmark comparisons
