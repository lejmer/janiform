## ISSUE-005: Threshold Policy And Pass Fail Decision Engine

### Description
Define and implement model-validation threshold policy plus pass-or-fail decision logic for scale-consistency checks across regimes.

### Acceptance Criteria
- Threshold policy is externalized and versioned
- Decision engine maps metric deltas to pass or fail outcomes per scenario
- Policy supports interval-dependent thresholds
- Decision output includes threshold IDs and rationale metadata
- Policy is suitable for release and calibration-governance decisions, not only engine-level sanity checks

### Unit tests
- Policy parsing and validation
- Boundary behavior at threshold cutoffs
- Deterministic decision outcomes on fixed inputs

### Notes/Constraints
- Keep policy governance aligned with broader calibration validation standards
- Avoid hardcoding thresholds in code paths
- Incorporate engine-level benchmark outputs from Initiative 004 as inputs rather than duplicating their ownership
