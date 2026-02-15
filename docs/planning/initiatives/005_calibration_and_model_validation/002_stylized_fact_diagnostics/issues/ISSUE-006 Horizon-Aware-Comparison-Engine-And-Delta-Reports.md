## ISSUE-006: Horizon Aware Comparison Engine And Delta Reports

### Description
Build comparison engine that aligns empirical and simulated diagnostics across horizons and computes delta reports with pass or fail signaling.

### Acceptance Criteria
- Engine compares metrics for matched horizons and emits delta summaries.
- Delta-report schema includes thresholds, status flags, and provenance metadata.
- Comparison supports batch evaluation across multiple calibration runs.
- Report outputs are machine-readable and human-readable.

### Unit tests
- Tests for horizon-alignment correctness and mismatch handling.
- Tests for delta computation and threshold classification.
- Tests for report schema and deterministic output ordering.

### Notes/Constraints
- Keep comparison logic independent from plotting or UI concerns.
- Avoid implicit horizon interpolation without explicit policy.
