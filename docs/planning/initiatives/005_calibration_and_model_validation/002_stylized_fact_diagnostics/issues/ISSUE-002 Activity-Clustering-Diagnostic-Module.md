## ISSUE-002: Activity Clustering Diagnostic Module

### Description
Implement diagnostics for event and volatility clustering behavior to compare empirical data against model-generated outputs.

### Acceptance Criteria
- Module computes clustering metrics from both historical and simulated datasets
- Metrics support configurable horizon buckets
- Output includes metric values and confidence or tolerance context
- Module is callable independently from calibration runs

### Unit tests
- Clustering metric correctness on controlled fixtures
- Horizon-bucket handling and edge cases
- Deterministic output on fixed fixtures

### Notes/Constraints
- Reuse shared timestamp and interval utilities
- Keep clustering metric computation separate from visualization code
