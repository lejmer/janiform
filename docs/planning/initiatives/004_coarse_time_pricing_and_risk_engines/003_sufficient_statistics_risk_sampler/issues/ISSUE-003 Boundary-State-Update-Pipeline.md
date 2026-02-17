## ISSUE-003: Boundary State Update Pipeline

### Description
Implement boundary state updates for the 4D Hawkes state using sampled interval statistics and canonical transition rules.

### Acceptance Criteria
- Pipeline updates boundary state deterministically per interval
- State update path is consistent with Hawkes kernel assumptions and oracle equations
- Full state carryover is preserved across multi-interval simulations
- Update failures report interval index and state snapshot metadata

### Unit tests
- Consistency with shared Hawkes transition APIs
- Boundary-state mapping correctness
- Deterministic state carryover across multiple intervals

### Notes/Constraints
- Keep state-update logic centralized and reusable
- Avoid duplicating state transitions in sampler wrappers
