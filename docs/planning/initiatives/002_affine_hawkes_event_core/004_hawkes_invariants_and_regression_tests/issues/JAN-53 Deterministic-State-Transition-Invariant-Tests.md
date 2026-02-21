## Deterministic State Transition Invariant Tests

### Description
Implement deterministic invariant tests for Hawkes state transitions covering drift, endogenous jumps, and exogenous loading updates.

### Acceptance Criteria
- Composed transition tests verify expected state evolution under fixed event schedules
- Failures include component-level mismatch diagnostics
- Behavior is deterministic and reproducible for identical inputs and configuration
- Deterministic schedules include irregular event-time gaps that verify transition correctness without fixed-grid assumptions

### Unit tests
- Drift-only invariant tests against closed-form decay references, Endogenous and exogenous jump-increment invariant tests, and Composed sequence tests for fixed transition schedules
- Deterministic irregular-timestamp schedule tests that mix boundary-adjacent and non-grid event times

### Notes/Constraints
- Keep fixtures small and interpretable for manual debugging
- Reuse kernel primitives from the canonical transition API
