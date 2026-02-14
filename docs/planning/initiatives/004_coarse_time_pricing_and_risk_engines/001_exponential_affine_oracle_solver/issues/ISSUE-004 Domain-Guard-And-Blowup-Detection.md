## ISSUE-004: Domain Guard And Blowup Detection

### Description
Implement domain guard checks for transform validity, including enforcement of `D dot r < 1 / theta_U` across the integration interval and controlled handling of blowup conditions.

### Acceptance Criteria
- Solver checks domain validity at runtime during integration steps.
- Violations produce structured failure outputs with checkpoint metadata.
- Blowup conditions terminate safely without undefined outputs.
- Guard behavior and threshold policies are documented.

### Unit tests
- Tests for valid scenarios that remain within domain.
- Tests for domain-violation scenarios with expected guarded failure.
- Tests for deterministic failure metadata content.

### Notes/Constraints
- Keep guard logic centralized in solver runtime path.
- Do not silently clamp violating states.
