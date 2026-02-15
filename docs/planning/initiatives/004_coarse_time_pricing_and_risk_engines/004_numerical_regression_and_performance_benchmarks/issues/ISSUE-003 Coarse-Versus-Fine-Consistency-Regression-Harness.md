## ISSUE-003: Coarse Versus Fine Consistency Regression Harness

### Description
Build regression harness that compares coarse-time outputs against controlled fine-scale Monte Carlo references for pricing and risk metrics.

### Acceptance Criteria
- Harness runs paired coarse and fine reference experiments on shared scenarios.
- Output includes metric deltas for key pricing and risk statistics.
- Regression pass or fail decisions use versioned tolerance configuration.
- Harness reports deterministic scenario identifiers and run metadata.

### Unit tests
- Tests for paired-experiment wiring and metric-delta calculations.
- Tests for pass or fail behavior using controlled fixture thresholds.
- Regression tests for deterministic harness report structure.

### Notes/Constraints
- Keep fine-scale references bounded to practical runtime budgets.
- Use shared random controls where appropriate to reduce variance noise.
