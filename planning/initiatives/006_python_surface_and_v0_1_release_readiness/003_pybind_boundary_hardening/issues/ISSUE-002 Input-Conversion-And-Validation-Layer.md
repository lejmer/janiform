## ISSUE-002: Input Conversion And Validation Layer

### Description
Implement robust conversion and validation at PyO3 boundaries so Python inputs are translated to core-domain types with clear error feedback.

### Acceptance Criteria
- Conversion layer translates Python inputs into validated core-domain request types
- Validation failures return consistent Python exceptions with actionable messages
- Conversion code remains thin and delegates business logic to core modules
- Conversion behavior is deterministic and documented

### Unit tests
- Invalid Python input types and value-domain failures and error-message consistency from conversion failures
- Valid input conversion across representative workflow calls

### Notes/Constraints
- Avoid putting domain logic inside conversion helpers
- Reuse existing core validators instead of re-implementing rules
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
