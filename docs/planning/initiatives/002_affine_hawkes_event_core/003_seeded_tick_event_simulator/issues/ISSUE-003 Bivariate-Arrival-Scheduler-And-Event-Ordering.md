## ISSUE-003: Bivariate Arrival Scheduler And Event Ordering

### Description
Implement the event scheduler for buy and sell arrivals under Hawkes intensities, with deterministic ordering guarantees for closely spaced or tied events.

### Acceptance Criteria
- Scheduler generates buy and sell arrivals using current Hawkes intensities and seasonality factors.
- Event ordering is deterministic for timestamp ties or near-ties.
- Scheduler integrates with shared state-transition operators for drift and jumps.
- Output ordering policy is documented and applied consistently.

### Unit tests
- Tests for strict monotonic or policy-defined event ordering.
- Tie-case tests that validate deterministic side ordering.
- Tests for interleaved buy and sell arrival scenarios.

### Notes/Constraints
- Keep ordering policy explicit so downstream diagnostics remain stable.
- Avoid duplicated scheduling logic in separate modules.
