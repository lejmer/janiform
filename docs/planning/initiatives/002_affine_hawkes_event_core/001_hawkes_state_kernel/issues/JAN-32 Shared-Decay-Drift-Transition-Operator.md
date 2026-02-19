## Shared Decay Drift Transition Operator

### Description
Implement deterministic drift evolution for the Hawkes state under shared exponential decay over a finite time step.

### Acceptance Criteria
- A drift operator updates all four state components using shared `beta` over `delta_t`
- Operator behavior is deterministic and independent of event arrival paths
- Edge-case behavior is defined for `delta_t = 0` and large `delta_t`
- Operator is reusable by both event simulator and coarse-time transform preparation paths

### Unit tests
- Analytic comparison tests for closed-form exponential decay on each component
- Edge-case tests for zero-duration and long-duration decay steps

### Notes/Constraints
- Keep floating-point behavior stable and documented for deterministic test fixtures
- Avoid embedding event-jump logic inside this operator
