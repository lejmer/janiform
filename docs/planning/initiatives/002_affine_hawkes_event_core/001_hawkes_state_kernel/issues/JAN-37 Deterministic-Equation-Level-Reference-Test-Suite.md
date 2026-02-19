## Deterministic Equation Level Reference Test Suite

### Description
Build a deterministic reference test suite for equation-level correctness of Hawkes state evolution under seeded scenarios and fixed transition schedules.

### Acceptance Criteria
- Reference scenarios cover drift-only, endogenous-event-only, exogenous-shock-only, and mixed update paths
- Test fixtures include expected component-level state values at selected checkpoints
- Seeded scenario execution is deterministic across repeated runs
- Test outputs provide component-level mismatch diagnostics for fast triage

### Unit tests
- Golden tests for fixed transition schedules and expected states and diagnostics tests ensuring mismatch reports include component and step identifiers
- Determinism tests for repeated seeded scenario runs

### Notes/Constraints
- Keep fixtures small and interpretable for manual verification
- Reuse this suite as a baseline for later performance and calibration regressions
