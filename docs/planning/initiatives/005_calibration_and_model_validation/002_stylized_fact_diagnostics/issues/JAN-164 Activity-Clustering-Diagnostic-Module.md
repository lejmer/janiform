## Activity Clustering Diagnostic Module

### Description
Implement diagnostics for event and volatility clustering behavior to compare empirical data against model-generated outputs.

### Acceptance Criteria
- Module computes clustering metrics from both historical and simulated datasets
- Metrics support configurable horizon buckets
- Output includes metric values, block-bootstrap confidence intervals, and threshold context
- Module enforces minimum sample-sufficiency checks and emits `insufficient_data` when coverage is inadequate
- Module is callable independently from calibration runs

### Unit tests
- Clustering metric correctness on controlled fixtures
- Horizon-bucket handling and edge cases
- Deterministic output on fixed fixtures
- Bootstrap-interval reproducibility under fixed seed and expected `insufficient_data` behavior on sparse horizons

### Notes/Constraints
- Reuse shared timestamp and interval utilities
- Keep clustering metric computation separate from visualization code
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
