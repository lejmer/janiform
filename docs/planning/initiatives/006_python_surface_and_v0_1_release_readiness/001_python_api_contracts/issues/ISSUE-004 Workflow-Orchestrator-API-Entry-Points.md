## ISSUE-004: Workflow Orchestrator API Entry Points

### Description
Implement top-level orchestrator functions for simulation, calibration, and pricing that call into shared core services through stable Python contracts.

### Acceptance Criteria
- Public entrypoints exist for each primary workflow and accept typed request models.
- Entrypoints return typed response models with optional diagnostics.
- Entrypoints are documented with usage examples.
- Entrypoints avoid embedding workflow-specific glue logic in multiple locations.

### Unit tests
- Integration tests for each public entrypoint on representative fixtures.
- Tests for request-validation failure behavior at entrypoint boundaries.
- Tests confirming typed response structure and required fields.

### Notes/Constraints
- Keep entrypoints thin and orchestration-focused.
- Reuse shared internal services for business logic execution.
