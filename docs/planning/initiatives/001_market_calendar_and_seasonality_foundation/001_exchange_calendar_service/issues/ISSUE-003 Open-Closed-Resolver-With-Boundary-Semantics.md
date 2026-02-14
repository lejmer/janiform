## ISSUE-003: Open Closed Resolver With Boundary Semantics

### Description
Implement the core resolver that returns market state as `open` or `closed` for a timestamp and exchange, with explicit boundary rules at session transitions.

### Acceptance Criteria
- Resolver API accepts timestamp and exchange ID and returns deterministic market state.
- Boundary semantics are explicitly defined for exact open and exact close timestamps.
- Resolver behavior is timezone-correct for all supported exchanges.
- The same resolver API is reusable by simulation, pricing, and calibration components.

### Unit tests
- Tests for pre-open, at-open, in-session, at-close, and post-close timestamps.
- Tests covering weekend timestamps and cross-day transitions.

### Notes/Constraints
- Keep the state vocabulary minimal for v0.1 using only `open` and `closed`.
- Document boundary policy in code-level docs and planning notes.
