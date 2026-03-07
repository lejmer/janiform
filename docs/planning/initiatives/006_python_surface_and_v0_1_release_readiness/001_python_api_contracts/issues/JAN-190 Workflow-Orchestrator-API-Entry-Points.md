## Workflow Orchestrator API Entry Points

### Description
Implement top-level orchestrator functions for simulation, calibration, and pricing that call into shared core services through explicit Python contracts.

### Acceptance Criteria
- Public entrypoints exist for each primary workflow and accept typed request models.
- Entrypoints return typed response models with optional diagnostics.
- Calibration entrypoints honor API default `execution_mode=memory` and support explicit override to `disk`.
- Entrypoint request and response docs use corrected known-surprise news semantics and open-jump versus closed-gap terminology where exposed.
- Entrypoints are documented with usage examples.
- Entrypoints avoid embedding workflow-specific glue logic in multiple locations.

### Unit tests
- Typed response structure and required fields.
- Request-validation failure behavior at entrypoint boundaries.
- Integration tests for each public entrypoint on representative fixtures.
- Calibration entrypoint tests for default mode behavior and override behavior.

### Notes/Constraints
- Keep entrypoints thin and orchestration-focused.
- Reuse shared internal services for business logic execution.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
