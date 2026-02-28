## ISSUE-005: API Contract Compatibility And Version Guardrails

### Description
Define API contract-evolution guardrails for pre-v0.1 development and implement checks that keep intentional breaking changes explicit, reviewed, and documented.

### Acceptance Criteria
- Contract-evolution policy defines what counts as a breaking change for public contracts
- Contract signatures and schema IDs are tracked with change-review checks
- Guardrail checks run in verification workflows and require explicit rationale for contract breaks
- Policy includes procedure for publishing updated contracts after approved breaking changes

### Unit tests
- Signature and schema snapshot checks for public APIs
- Tests that detect undocumented contract changes and accept documented, approved contract breaks

### Notes/Constraints
- Keep contract-evolution checks simple and reliable
- Avoid blocking internal refactors that do not affect public contracts
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
