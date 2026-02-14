## ISSUE-005: Acceptance Thresholds And Factor Range Policies

### Description
Define quantitative acceptance thresholds for deterministic outputs and expected seasonality factor ranges, including policy for threshold updates.

### Acceptance Criteria
- Threshold configuration exists for state classification strictness and factor-range checks.
- Factor-range policy defines acceptable bounds for `s_h` and `nu_h` per fixture class.
- Threshold changes are versioned and recorded with rationale.
- Validation harness consumes threshold configuration without code changes.

### Unit tests
- Tests for threshold configuration parsing and schema validation.
- Tests verifying pass and fail behavior at threshold boundaries.
- Regression tests for factor-range policy application on canonical fixtures.

### Notes/Constraints
- Keep thresholds conservative enough to catch regressions while avoiding noisy failures.
- Centralize threshold configuration to prevent drift between test suites.
