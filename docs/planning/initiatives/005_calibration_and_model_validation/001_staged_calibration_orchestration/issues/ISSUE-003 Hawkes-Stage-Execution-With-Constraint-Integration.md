## ISSUE-003: Hawkes Stage Execution With Constraint Integration

### Description
Implement Hawkes-stage calibration execution with integrated parameter and stability constraint enforcement during optimization.

### Acceptance Criteria
- Stage runner executes Hawkes fit using constrained parameterization and validation hooks.
- Positivity and stability checks are enforced during candidate evaluation.
- Stage emits fitted Hawkes parameters with diagnostics and failure details.
- Stage accepts upstream seasonality artifacts as deterministic inputs.

### Unit tests
- Tests for successful Hawkes-stage runs under valid fixtures.
- Tests for constraint-violation handling and failure reporting.
- Tests for deterministic outputs under fixed seed and optimizer settings.

### Notes/Constraints
- Reuse constraint modules from Initiative 002.
- Avoid duplicating optimizer-constraint wiring logic.
