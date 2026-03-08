## Scenario Schema And Data Driven Case Definitions

### Description
Define a data-driven scenario schema for open and closed gap validation cases, including inputs, expected outcomes, and exchange context.

### Acceptance Criteria
- Scenario loader validates required fields and reports structured errors
- Scenario definitions are reusable across engines and test suites
- Output schema and metadata are machine-readable and documented

### Unit tests
- Schema validation tests for valid and invalid scenario payloads
- Loader tests for deterministic ordering and parsing behavior

### Notes/Constraints
- Keep schema exchange-agnostic and extensible
- Avoid hardcoded scenario logic in test code
