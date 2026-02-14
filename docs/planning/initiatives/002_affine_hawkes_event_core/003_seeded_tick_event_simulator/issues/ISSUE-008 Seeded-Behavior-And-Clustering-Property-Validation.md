## ISSUE-008: Seeded Behavior And Clustering Property Validation

### Description
Create validation tests that confirm deterministic seeded behavior and expected clustering and asymmetry signatures under representative parameter regimes.

### Acceptance Criteria
- Seeded runs are deterministic under repeated execution.
- Validation scenarios demonstrate burst clustering behavior in event arrivals.
- Validation scenarios demonstrate expected buy and sell asymmetry under asymmetric parameter sets.
- Test outputs include summary metrics and thresholds for pass and fail decisions.

### Unit tests
- Determinism tests for repeated seeded runs.
- Statistical-property tests for clustering metrics on controlled fixtures.
- Asymmetry tests for side-specific behavior under structured parameter choices.

### Notes/Constraints
- Keep thresholds robust enough to avoid flaky outcomes.
- Separate strict deterministic checks from statistical tolerance checks.
