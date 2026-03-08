## Reload Resolver And Compatibility Checks

### Description
Implement reload resolver that reconstructs calibration runs from persisted artifacts with schema-evolution checks and clear failure diagnostics.

### Acceptance Criteria
- Resolver can load complete runs and stage-specific subsets from persisted artifacts.
- Schema-evolution checks validate schema version and required component availability.
- Reload API returns structured errors for unsupported-schema or corrupted artifacts.
- Reload output is usable by validation and pricing workflows.
- Reload behavior documentation explicitly references persistence defaults: artifacts are optional, ON by default in CLI flows, and OFF by default for imported package usage unless explicitly enabled.
- Reload behavior is scoped to runs where `execution_mode=disk` and artifacts exist.

### Unit tests
- Successful reload of complete and partial artifact sets.
- Unsupported-schema handling.
- Corrupted artifact detection and error reporting.

### Notes/Constraints
- Keep reload path decoupled from calibration execution side effects.
- Ensure deterministic object reconstruction order.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
