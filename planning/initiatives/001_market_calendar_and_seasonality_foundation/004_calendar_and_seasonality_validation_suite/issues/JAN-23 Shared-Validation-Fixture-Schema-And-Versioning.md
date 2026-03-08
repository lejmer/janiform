## Shared Validation Fixture Schema And Versioning

### Description
Define a canonical schema and versioning strategy for calendar and seasonality validation fixtures shared between Rust and Python tests.

### Acceptance Criteria
- Fixture loading behavior is deterministic across repeated runs
- Output schema and metadata are machine-readable and documented

### Unit tests
- Positive tests for valid fixture documents with multiple exchanges and determinism tests for fixture loading order and content hashing
- Schema validation tests for missing required fields and invalid values

### Notes/Constraints
- Keep schema stable and forward-extensible for v0.x updates
- Avoid embedding language-specific fields that only one test stack can use
