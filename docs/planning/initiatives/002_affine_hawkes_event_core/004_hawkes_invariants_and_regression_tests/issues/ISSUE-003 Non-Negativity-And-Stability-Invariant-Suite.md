## ISSUE-003: Non Negativity And Stability Invariant Suite

### Description
Build invariant tests that enforce non-negativity and stability conditions under valid parameter regimes and state updates.

### Acceptance Criteria
- Tests verify no negative state increments are introduced by valid transition operations
- Tests verify intensity projections remain valid under positive baseline and seasonality inputs
- Stability-oriented scenarios include parameter sets near and within valid spectral-radius bounds
- Failures clearly identify whether the violation is parameter, transition, or projection related

### Unit tests
- Projection sanity tests for de-seasonalized and scaled intensities
- Stability-boundary tests for regimes near `rho(Alpha / beta) < 1` threshold
- Non-negativity invariant tests across representative valid parameter fixtures

### Notes/Constraints
- Keep test assumptions aligned with documented parameter validators
- Separate invalid-parameter tests from valid-regime invariants
