## Seasonality To Hourly s_h Materializer

### Description
Implement materialization from continuous or high-resolution seasonality signal `s(t)` into hourly piecewise-constant `s_h` values for each modeled interval.

### Acceptance Criteria
- Materializer accepts seasonality source and target range and emits one `s_h` value per hour bucket
- Materializer handles sparse source segments using documented fallback behavior
- Behavior is deterministic and reproducible for identical inputs and configuration
- Interpolation and aggregation choices are captured in output metadata for auditability

### Unit tests
- Sparse or missing source segments with expected fallback behavior
- Deterministic `s_h` output on controlled synthetic `s(t)` inputs and reproducibility under fixed configuration

### Notes/Constraints
- Keep interpolation strategy configurable but bounded to approved methods
- Reuse shared interval indexing utilities from the contract layer
