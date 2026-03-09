## Shared News Direct Jump Generation For Coarse Intervals

### Description
Implement direct jump generation for the coarse simulation workflow so open-window `M` and closed-window `G` increments are produced inside the engine from shared news-process inputs rather than supplied externally by the caller.

The current interval return generator can route `open_jump_return` and `closed_gap_return`, but it expects those values to be provided from outside the workflow. That leaves the coarse simulation path incomplete relative to the model design, where news arrivals, Hawkes excitation loading, and direct return jumps are coupled parts of one process.

### Acceptance Criteria
- Coarse simulation model inputs include the news-process and coupled-mark parameters required to generate direct jump increments.
- Open-window direct jumps and closed-window gap jumps are generated from the same shared news semantics used for Hawkes excitation.
- Market-state routing semantics for `M` and `G` are explicit and consistent with calendar-aware open and closed interval behavior.
- Coarse simulation no longer depends on callers supplying precomputed direct-jump return increments per interval.
- Generated jump components remain visible in interval-level return decomposition outputs for diagnostics and validation.

### Unit tests
- Deterministic seeded tests for repeated coarse direct-jump generation under identical inputs.
- Routing tests showing open-window news contributes to `M` and closed-window news contributes to `G`.
- Failure-path tests for invalid mark-law or coupled-gap configuration inputs.

### Notes/Constraints
- Reuse the shared news-process and dispatch building blocks already present in the Rust core.
- Keep direct-jump generation aligned with the corrected known-surprise news semantics and avoid introducing a second incompatible jump model for coarse simulation.
