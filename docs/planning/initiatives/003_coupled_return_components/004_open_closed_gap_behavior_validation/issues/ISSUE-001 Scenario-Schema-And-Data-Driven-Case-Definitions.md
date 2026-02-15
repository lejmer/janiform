## ISSUE-001: Scenario Schema And Data Driven Case Definitions

### Description
Define a data-driven scenario schema for open and closed gap validation cases, including inputs, expected outcomes, and exchange context.

### Acceptance Criteria
- A scenario schema exists for event timing, market state, marks, and expected component outcomes.
- Schema supports overnight and weekend baseline cases plus intraday injected-news case definitions.
- Scenario loader validates required fields and reports structured errors.
- Scenario definitions are reusable across engines and test suites.

### Unit tests
- Schema validation tests for valid and invalid scenario payloads.
- Loader tests for deterministic ordering and parsing behavior.

### Notes/Constraints
- Keep schema exchange-agnostic and extensible.
- Avoid hardcoded scenario logic in test code.
