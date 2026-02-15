## ISSUE-005: Threshold Policy And Pass Fail Decision Engine

### Description
Define and implement threshold policy plus pass-or-fail decision logic for scale-consistency checks.

### Acceptance Criteria
- Threshold policy is externalized and versioned.
- Decision engine maps metric deltas to pass or fail outcomes per scenario.
- Policy supports interval-dependent thresholds.
- Decision output includes threshold IDs and rationale metadata.

### Unit tests
- Tests for policy parsing and validation.
- Tests for boundary behavior at threshold cutoffs.
- Tests for deterministic decision outcomes on fixed inputs.

### Notes/Constraints
- Keep policy governance aligned with broader calibration validation standards.
- Avoid hardcoding thresholds in code paths.
