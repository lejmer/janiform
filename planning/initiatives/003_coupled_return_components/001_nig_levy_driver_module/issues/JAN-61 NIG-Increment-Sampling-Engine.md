## NIG Increment Sampling Engine

### Description
Implement sampling utilities for NIG increments in business time so path simulation can generate return moves conditional on integrated intensity.

### Acceptance Criteria
- Sampler accepts NIG parameters and business-time increment scale
- Sampler supports deterministic seeded execution through shared randomness interfaces
- Sampling API is reusable in path simulation and diagnostic workflows
- Sampler behavior is documented for edge cases such as near-zero time increment

### Unit tests
- Basic distribution sanity tests on sampled increments
- Edge-case tests for very small and moderate time increments
- Seed reproducibility tests for repeated sampling runs

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Reuse project-wide RNG abstractions for reproducibility
- Keep sampling implementation isolated from pricing-only code
