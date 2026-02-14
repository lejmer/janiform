## ISSUE-002: Deterministic State Transition Invariant Tests

### Description
Implement deterministic invariant tests for Hawkes state transitions covering drift, endogenous jumps, and exogenous loading updates.

### Acceptance Criteria
- Deterministic tests verify equation-level correctness for each transition operator.
- Composed transition tests verify expected state evolution under fixed event schedules.
- Tests assert deterministic behavior for repeated execution with identical fixtures.
- Failures include component-level mismatch diagnostics.

### Unit tests
- Drift-only invariant tests against closed-form decay references.
- Endogenous and exogenous jump-increment invariant tests.
- Composed sequence tests for fixed transition schedules.

### Notes/Constraints
- Keep fixtures small and interpretable for manual debugging.
- Reuse kernel primitives from the canonical transition API.
