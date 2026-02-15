## ISSUE-003: Error Taxonomy And Exception Contract

### Description
Define Python-facing exception taxonomy and error contract so workflow failures are consistent, actionable, and stable for callers.

### Acceptance Criteria
- Error classes are defined for validation, execution, and integration failures.
- Exception mapping includes structured context and stable error identifiers.
- Public API docs describe expected failure modes per workflow.
- Error contract remains consistent across CLI and importable API usage.

### Unit tests
- Tests for expected exception type per failure scenario.
- Tests for required error metadata fields and message formatting.
- Tests for stability of error identifiers across versions.

### Notes/Constraints
- Keep user-facing errors clear and non-leaky.
- Avoid exposing raw internal exceptions through public APIs.
