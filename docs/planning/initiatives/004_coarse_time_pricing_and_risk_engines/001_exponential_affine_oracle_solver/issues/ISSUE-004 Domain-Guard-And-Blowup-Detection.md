## ISSUE-004: Domain Guard And Blowup Detection

### Description
Implement domain guard checks for transform validity, including enforcement of `D dot r < 1 / theta_U` across the integration interval and controlled handling of blowup conditions.

### Acceptance Criteria
- Solver checks domain validity at runtime during integration steps
- Blowup conditions terminate safely without undefined outputs
- Output schema and metadata are machine-readable and documented

### Unit tests
- Valid scenarios that remain within domain
- Domain-violation scenarios with expected guarded failure and deterministic failure metadata content

### Notes/Constraints
- Keep guard logic centralized in solver runtime path
- Do not silently clamp violating states
