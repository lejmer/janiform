## ISSUE-004: Hour Boundary Carryover And Ordering Regression Scenarios

### Description
Add regression scenarios focused on event ordering and full 4D state carryover across hour boundaries and interval transitions.

### Acceptance Criteria
- Regression scenarios cover boundary-adjacent events before and after interval edges.
- Carryover checks assert exact transfer of `Y++`, `Y+-`, `Y-+`, and `Y--` across hour boundaries.
- Event ordering policy is verified in tied or near-tied timestamp scenarios.
- Scenario outputs are stable under repeated seeded execution.

### Unit tests
- Boundary carryover tests for multi-hour fixed-seed runs.
- Ordering regression tests for tie and near-tie events.
- Determinism tests for repeated boundary scenarios.

### Notes/Constraints
- Reuse calendar and factor fixtures from Initiative 001 where applicable.
- Keep ordering policy references synchronized with simulator docs.
