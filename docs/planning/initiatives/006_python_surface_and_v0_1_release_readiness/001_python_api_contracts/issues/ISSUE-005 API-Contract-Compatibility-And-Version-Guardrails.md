## ISSUE-005: API Contract Compatibility And Version Guardrails

### Description
Define compatibility guardrails for v0.1 API contracts and implement checks that prevent accidental breaking changes to stable signatures.

### Acceptance Criteria
- Compatibility policy defines what counts as breaking change for public contracts.
- Contract signatures are tracked with version-aware checks.
- Guardrail checks run in verification workflows.
- Policy includes procedure for introducing experimental endpoints.

### Unit tests
- Tests for signature compatibility snapshots on stable APIs.
- Tests that detect intentional contract-breaking modifications.
- Tests for experimental endpoint isolation from stable contract checks.

### Notes/Constraints
- Keep compatibility checks simple and reliable.
- Avoid blocking internal refactors that do not affect public contracts.
