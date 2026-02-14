## ISSUE-004: Closed Window Behavior Explicit Gap Emission

### Description
Implement targeted validation checks that confirm explicit gap increments are emitted for closed-window news events and coupled correctly with activity updates.

### Acceptance Criteria
- Closed-window scenarios produce non-zero `G` contributions when marks imply gap jumps.
- Closed-window scenarios confirm `J_t` and `G_t` are updated from the same news events.
- Validation captures expected relationship between shared marks and emitted gap increments.
- Failures include event-level diagnostics for gap emission mismatches.

### Unit tests
- Deterministic closed-window scenarios with expected gap emission outcomes.
- Tests validating shared-mark coupling between `J_t` and `G_t` updates.
- Regression tests for missing or misrouted gap contributions.

### Notes/Constraints
- Keep coupling checks aligned with shared dispatch implementation.
- Avoid statistical-only assertions for deterministic scenario checks.
