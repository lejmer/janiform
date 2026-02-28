## ISSUE-006: Horizon Aware Comparison Engine And Delta Reports

### Description
Build comparison engine that aligns empirical and simulated diagnostics across horizons and computes delta reports with pass or fail signaling.

### Acceptance Criteria
- Engine compares metrics for matched horizons and emits delta summaries
- Comparison supports batch evaluation across multiple calibration runs
- Output schema and metadata are machine-readable and documented

### Unit tests
- Horizon-alignment correctness and mismatch handling and delta computation and threshold classification
- Report schema and deterministic output ordering

### Notes/Constraints
- Keep comparison logic independent from plotting or UI concerns
- Avoid implicit horizon interpolation without explicit policy
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
