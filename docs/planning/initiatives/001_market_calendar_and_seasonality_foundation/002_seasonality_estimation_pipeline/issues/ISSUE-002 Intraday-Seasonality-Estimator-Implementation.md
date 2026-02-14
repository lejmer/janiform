## ISSUE-002: Intraday Seasonality Estimator Implementation

### Description
Implement the nonparametric intraday estimator for `f_intra(tod)` using event timestamp density over time of day. The estimator should support smooth output suitable for hourly aggregation later.

### Acceptance Criteria
- Estimator computes an intraday component from preprocessed events without assuming a fixed U-shape.
- Smoothing configuration is explicit and can be controlled through estimator settings.
- Output is stable and reproducible for fixed inputs and settings.
- Estimator API is model-agnostic and does not depend on Hawkes internals.

### Unit tests
- Tests for expected behavior on synthetic intraday patterns.
- Tests for sparse intraday bins and edge handling near session boundaries.
- Reproducibility tests under fixed configuration.

### Notes/Constraints
- Keep estimator outputs machine-readable for downstream composition.
- Avoid hardcoding exchange-specific assumptions in estimator logic.
