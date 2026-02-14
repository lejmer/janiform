## ISSUE-002: Gamma Severity Mark Sampler

### Description
Implement severity mark sampling `U ~ Gamma(k_U, theta_U)` for news arrivals with deterministic seeded behavior and domain validation.

### Acceptance Criteria
- Sampler supports valid positive shape and scale parameter inputs.
- Sampling output is non-negative and deterministic under fixed seeds.
- API is reusable for both Hawkes loading and gap-mark scaling.
- Invalid distribution parameters fail fast with structured errors.

### Unit tests
- Tests for parameter-domain validation and error behavior.
- Seed reproducibility tests for repeated sampling runs.
- Basic distribution sanity tests on sampled severity values.

### Notes/Constraints
- Reuse shared RNG abstraction for cross-module determinism.
- Keep sampler implementation decoupled from arrival-process logic.
