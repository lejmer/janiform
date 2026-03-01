## Tail And Skew Diagnostic Module

### Description
Implement heavy-tail and skew diagnostics across multiple horizons for empirical and simulated return series.

### Acceptance Criteria
- Module computes robust primary tail-heaviness and skew metrics across configured horizons using quantile-shape and tail-risk formulations
- Module reports L-moment and classical moment diagnostics as secondary outputs for interpretability
- Output supports side-by-side model versus data comparison
- Metric calculations include guardrails for sparse tail samples
- Module emits structured metrics with metadata on sample coverage, uncertainty intervals, and `insufficient_data` status

### Unit tests
- Tail and skew metric correctness on synthetic distributions and sparse-sample handling and fallback behavior
- Metric stability across repeated runs
- Cross-check tests verify robust primary metrics remain stable relative to classical skew or kurtosis under outlier stress fixtures

### Notes/Constraints
- Keep metric calculations numerically stable near extreme quantiles
- Avoid embedding model-specific assumptions in generic metrics
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
