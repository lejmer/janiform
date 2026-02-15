## ISSUE-003: Domain Error To Python Exception Mapping

### Description
Create a structured mapping from core-domain errors to stable Python exception classes and messages.

### Acceptance Criteria
- Error taxonomy mapping is defined for validation, runtime, and internal failure categories.
- Mapping produces consistent Python exception classes across entrypoints.
- Error payload includes stable identifiers and actionable context.
- Mapping behavior is documented for users and maintainers.

### Unit tests
- Tests for expected exception type per domain-error category.
- Tests for required error payload fields and message format.
- Regression tests for mapping stability across module entrypoints.

### Notes/Constraints
- Keep exception surface small and predictable.
- Do not expose raw internal error internals in public exceptions.
