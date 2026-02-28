## ISSUE-006: Boundary Edge Case Test Suite And Triage Diagnostics

### Description
Build a targeted boundary test suite for conversion edge cases and failure-path mapping, with diagnostics that speed root-cause triage.

### Acceptance Criteria
- Test suite covers edge cases for malformed inputs, domain violations, and internal failures
- Tests validate exception class, message shape, and documented error IDs
- Failure diagnostics include entrypoint and boundary-stage context
- Test fixtures are reusable across future pybind entrypoints

### Unit tests
- Stable exception surface behavior
- Edge-case tests for each exposed entrypoint argument pattern and failure-path tests for conversion, dispatch, and panic-guard branches

### Notes/Constraints
- Keep suite focused on boundary behavior, not deep algorithm validation
- Ensure failures are concise and actionable for maintainers
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
