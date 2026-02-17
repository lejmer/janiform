## ISSUE-003: Tail And Skew Diagnostic Module

### Description
Implement heavy-tail and skew diagnostics across multiple horizons for empirical and simulated return series.

### Acceptance Criteria
- Module computes tail-heaviness and skew metrics across configured horizons
- Output supports side-by-side model versus data comparison
- Metric calculations include guardrails for sparse tail samples
- Module emits structured metrics with metadata on sample coverage

### Unit tests
- Tail and skew metric correctness on synthetic distributions and sparse-sample handling and fallback behavior
- Metric stability across repeated runs

### Notes/Constraints
- Keep metric calculations numerically stable near extreme quantiles
- Avoid embedding model-specific assumptions in generic metrics
