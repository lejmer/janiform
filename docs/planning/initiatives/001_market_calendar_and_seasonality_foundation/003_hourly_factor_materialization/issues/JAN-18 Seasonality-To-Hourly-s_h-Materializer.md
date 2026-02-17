## Seasonality To Hourly s_h Materializer

### Description
Implement materialization from continuous or high-resolution seasonality signal `s(t)` into hourly piecewise-constant `s_h` values for each modeled interval.

### Acceptance Criteria
- Materializer accepts seasonality source and target range and emits one `s_h` value per hour bucket.
- Interpolation or aggregation method is explicit, deterministic, and recorded in output metadata.
- Output is reproducible for identical inputs and configuration.
- Materializer handles sparse source segments using documented fallback behavior.

### Unit tests
- Tests for deterministic `s_h` output on controlled synthetic `s(t)` inputs.
- Tests for sparse or missing source segments with expected fallback behavior.
- Regression tests for reproducibility under fixed configuration.

### Notes/Constraints
- Keep interpolation strategy configurable but bounded to approved methods.
- Reuse shared interval indexing utilities from the contract layer.
