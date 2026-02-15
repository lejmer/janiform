## ISSUE-007: Deterministic Seeding And Reproducibility Controls

### Description
Implement deterministic seeding policy and reproducibility controls for stage execution, optimization routines, and sampling-based objective terms.

### Acceptance Criteria
- Seed policy is explicit for global run and per-stage derived seeds.
- Repeated runs with identical inputs and settings produce reproducible outputs.
- Seed metadata is captured in run artifacts and diagnostics.
- Reproducibility controls are applied consistently across all stages.

### Unit tests
- Tests for repeated-run determinism across full staged workflow.
- Tests for per-stage seed derivation determinism.
- Tests for metadata capture of run and stage seed values.

### Notes/Constraints
- Reuse existing RNG abstractions where available.
- Avoid hidden nondeterministic sources in stage adapters.
