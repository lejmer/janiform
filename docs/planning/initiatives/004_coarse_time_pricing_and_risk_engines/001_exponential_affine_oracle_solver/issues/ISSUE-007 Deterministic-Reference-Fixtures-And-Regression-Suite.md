## ISSUE-007: Deterministic Reference Fixtures And Regression Suite

### Description
Create deterministic reference fixtures for solver inputs and expected outputs, then build a regression suite that protects against equation and integration drift.

### Acceptance Criteria
- Fixture set includes representative stable scenarios for one-interval solver runs.
- Expected outputs include `C(Delta)`, `D(Delta)`, and transform values with tolerance definitions.
- Regression suite fails on drift beyond configured tolerances.
- Fixture update workflow is documented.

### Unit tests
- Regression tests against reference solver outputs.
- Tests for fixture-schema validation and required metadata.
- Determinism tests for repeated solver runs on fixed fixtures.

### Notes/Constraints
- Separate deterministic fixture tests from stochastic downstream tests.
- Keep fixture set compact for fast local and CI execution.
