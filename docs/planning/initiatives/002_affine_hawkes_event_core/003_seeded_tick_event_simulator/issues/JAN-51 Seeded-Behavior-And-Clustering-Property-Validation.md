## Seeded Behavior And Clustering Property Validation

### Description
Create validation tests that confirm deterministic seeded behavior and expected clustering and asymmetry signatures under representative parameter regimes.

### Acceptance Criteria
- Seeded runs are deterministic under repeated execution
- Validation scenarios demonstrate burst clustering behavior in event arrivals
- Validation scenarios demonstrate expected buy and sell asymmetry under asymmetric parameter sets
- Test outputs include summary metrics and thresholds for pass and fail decisions
- Validation includes scale-consistency checks where aggregated tick-path summaries are compared against direct coarse-horizon summaries under shared model inputs for representative horizons such as 15m, 1h, and 1d
- Validation scenarios include non-grid event-time fixtures so clustering diagnostics are not tied to fixed timestamp lattices

### Unit tests
- Statistical-property tests for clustering metrics on controlled fixtures and asymmetry tests for side-specific behavior under structured parameter choices
- Determinism tests for repeated seeded runs
- Coarse-vs-aggregated comparison tests with explicit tolerance bands and diagnostics

### Notes/Constraints
- Keep thresholds robust enough to avoid flaky outcomes
- Separate strict deterministic checks from statistical tolerance checks
