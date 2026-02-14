## ISSUE-003: NIG Increment Sampling Engine

### Description
Implement sampling utilities for NIG increments in business time so path simulation can generate return moves conditional on integrated intensity.

### Acceptance Criteria
- Sampler accepts NIG parameters and business-time increment scale.
- Sampler supports deterministic seeded execution through shared randomness interfaces.
- Sampling API is reusable in path simulation and diagnostic workflows.
- Sampler behavior is documented for edge cases such as near-zero time increment.

### Unit tests
- Seed reproducibility tests for repeated sampling runs.
- Basic distribution sanity tests on sampled increments.
- Edge-case tests for very small and moderate time increments.

### Notes/Constraints
- Reuse project-wide RNG abstractions for reproducibility.
- Keep sampling implementation isolated from pricing-only code.
