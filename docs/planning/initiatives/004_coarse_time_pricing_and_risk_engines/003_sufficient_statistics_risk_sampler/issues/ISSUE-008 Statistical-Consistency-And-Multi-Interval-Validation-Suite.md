## ISSUE-008: Statistical Consistency And Multi Interval Validation Suite

### Description
Build validation suite for statistical consistency and state-evolution behavior across multi-interval risk runs.

### Acceptance Criteria
- Validation scenarios cover short, medium, and long horizon interval runs.
- Suite checks state-evolution consistency and key distributional sanity metrics.
- Regression thresholds are defined and versioned.
- Validation reports are machine-readable and comparable across runs.

### Unit tests
- Multi-interval consistency tests for sampled statistics and state updates.
- Statistical sanity tests with tolerance-based pass or fail criteria.
- Regression tests for expected output stability under fixed fixtures.

### Notes/Constraints
- Keep stochastic checks robust to avoid flaky failures.
- Separate deterministic invariants from statistical tolerance checks.
