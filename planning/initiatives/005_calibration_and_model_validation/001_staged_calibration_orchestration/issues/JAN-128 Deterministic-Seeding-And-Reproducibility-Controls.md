## Deterministic Seeding And Reproducibility Controls

### Description
Implement deterministic seeding policy and reproducibility controls for stage execution, optimization routines, and sampling-based objective terms across `memory` and `disk` execution modes.

### Acceptance Criteria
- Seed policy is explicit for global run and per-stage derived seeds.
- Repeated runs with identical inputs and settings produce reproducible outputs.
- Seed metadata is captured in run results for both modes and in persisted artifacts for `disk` mode.
- Reproducibility controls are applied consistently across all stages.

### Unit tests
- Per-stage seed derivation determinism and metadata capture of run and stage seed values.
- Repeated-run determinism across full staged workflow in both execution modes.

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Reuse existing RNG abstractions where available.
- Avoid hidden nondeterministic sources in stage adapters.
- Follow the combined run-contract guidance in JAN-122 and JAN-130.
