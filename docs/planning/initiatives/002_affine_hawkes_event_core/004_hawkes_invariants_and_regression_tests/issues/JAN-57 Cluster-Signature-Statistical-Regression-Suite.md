## Cluster Signature Statistical Regression Suite

### Description
Implement statistical regression checks for clustered-event signatures to guard against accidental erosion of Hawkes burst dynamics.

### Acceptance Criteria
- Statistical checks compute agreed clustering metrics on seeded simulation batches
- Baseline reference bands are defined for core clustering metrics
- Regression failures report metric deltas relative to reference bands
- Statistical suite runs separately from strict deterministic equation tests
- Statistical checks include a scale-consistency panel comparing aggregated tick metrics with direct coarse-horizon metrics for matching fixtures, including representative horizons such as 15m, 1h, and 1d

### Unit tests
- Metric-calculation tests on synthetic clustered and non-clustered fixtures
- Reproducibility tests for batch metric outputs under fixed seeds
- Regression tests validating pass and fail behavior against reference bands
- Regression tests validating pass and fail behavior for coarse-vs-aggregated consistency bands

### Notes/Constraints
- Keep sample sizes bounded to maintain reasonable runtime
- Use tolerance bands rather than exact-value assertions for stochastic summaries
- Keep consistency-band definitions explicit and versioned to avoid silent drift
