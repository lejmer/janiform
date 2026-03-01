## ISSUE-006: Horizon Aware Comparison Engine And Delta Reports

### Description
Build comparison engine that aligns empirical and simulated diagnostics across horizons and computes delta reports with pass or fail signaling.

### Acceptance Criteria
- Engine compares metrics for matched horizons and emits delta summaries
- Engine supports uncertainty-aware comparison outputs, including block-bootstrap confidence intervals and standardized deltas where applicable
- Engine applies minimum sample-sufficiency policy and emits explicit `insufficient_data` decisions instead of forced pass or fail
- Comparison supports batch evaluation across multiple calibration runs
- Output schema and metadata are machine-readable and documented

### Unit tests
- Horizon-alignment correctness and mismatch handling and delta computation and threshold classification
- Report schema and deterministic output ordering
- Decision-state tests cover pass, fail, and `insufficient_data` outcomes under fixed fixtures

### Notes/Constraints
- Keep comparison logic independent from plotting or UI concerns
- Avoid implicit horizon interpolation without explicit policy
- Treat empirical diagnostics as noisy estimates in decision logic, not exact calibration targets
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
