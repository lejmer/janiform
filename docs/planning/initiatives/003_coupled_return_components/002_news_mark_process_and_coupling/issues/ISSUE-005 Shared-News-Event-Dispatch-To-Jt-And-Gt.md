## ISSUE-005: Shared News Event Dispatch To Jt And Gt

### Description
Implement a shared dispatch pipeline where each news event updates Hawkes shock accumulator `J_t` and, conditionally, gap process `G_t` without duplicated event handling.

### Acceptance Criteria
- One event-dispatch path updates `J_t` using severity marks for every news event.
- Gap process update uses the same event and mark context, gated by market state rules.
- Dispatch ordering is deterministic and documented.
- Pipeline outputs are reusable by simulation and coarse-time engines.

### Unit tests
- Tests verifying one-to-one mapping between news events and `J_t` updates.
- Tests verifying coupled `J_t` and `G_t` updates under closed windows.
- Tests for deterministic dispatch ordering under seeded scenarios.

### Notes/Constraints
- Keep dispatch logic free of pricing-only concerns.
- Avoid separate code paths that could desynchronize `J_t` and `G_t` coupling.
