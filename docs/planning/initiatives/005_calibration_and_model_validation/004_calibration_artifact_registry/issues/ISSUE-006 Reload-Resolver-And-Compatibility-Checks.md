## ISSUE-006: Reload Resolver And Compatibility Checks

### Description
Implement reload resolver that reconstructs calibration runs from artifacts with compatibility checks and clear failure diagnostics.

### Acceptance Criteria
- Resolver can load complete runs and stage-specific subsets from artifacts.
- Compatibility checks validate schema version and required component availability.
- Reload API returns structured errors for incompatible or corrupted artifacts.
- Reload output is usable by validation and pricing workflows.

### Unit tests
- Tests for successful reload of complete and partial artifact sets.
- Tests for schema-version incompatibility handling.
- Tests for corrupted artifact detection and error reporting.

### Notes/Constraints
- Keep reload path decoupled from calibration execution side effects.
- Ensure deterministic object reconstruction order.
