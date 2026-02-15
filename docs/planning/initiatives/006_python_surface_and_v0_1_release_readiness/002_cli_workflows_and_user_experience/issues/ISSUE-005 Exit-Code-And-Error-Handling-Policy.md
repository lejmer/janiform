## ISSUE-005: Exit Code And Error Handling Policy

### Description
Define stable exit-code and error-handling policy so CLI behavior is predictable in automated scripts and CI workflows.

### Acceptance Criteria
- Exit-code map is documented for success, validation errors, runtime errors, and internal failures.
- Commands return deterministic exit codes for equivalent failure classes.
- Error output includes actionable context without leaking internals.
- Error behavior is consistent across all primary commands.

### Unit tests
- Tests for expected exit codes in nominal and failure scenarios.
- Tests for error-message formatting and required context fields.
- Tests for consistent failure classification across commands.

### Notes/Constraints
- Keep exit-code map small and stable.
- Avoid command-specific one-off error semantics.
