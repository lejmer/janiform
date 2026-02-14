## ISSUE-001: Shared Validation Fixture Schema And Versioning

### Description
Define a canonical schema and versioning strategy for calendar and seasonality validation fixtures shared between Rust and Python tests.

### Acceptance Criteria
- A documented fixture schema exists for sessions, holidays, event windows, and expected factor outputs.
- Fixture format includes version metadata and exchange identifiers.
- Schema validation rejects malformed fixtures with actionable errors.
- Fixture loading behavior is deterministic across repeated runs.

### Unit tests
- Schema validation tests for missing required fields and invalid values.
- Positive tests for valid fixture documents with multiple exchanges.
- Determinism tests for fixture loading order and content hashing.

### Notes/Constraints
- Keep schema stable and forward-extensible for v0.x updates.
- Avoid embedding language-specific fields that only one test stack can use.
