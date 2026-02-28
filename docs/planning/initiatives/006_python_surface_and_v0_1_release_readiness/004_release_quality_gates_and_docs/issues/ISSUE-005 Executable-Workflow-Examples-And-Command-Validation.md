## ISSUE-005: Executable Workflow Examples And Command Validation

### Description
Publish practical v0.1 workflow examples and validate that documented commands remain executable against the current repository.

### Acceptance Criteria
- Examples cover simulation, calibration, and pricing day-one workflows
- Example commands are validated in automated checks or scripted smoke runs
- Output expectations are documented at a high level
- Example paths are kept in sync with CLI and API contracts

### Unit tests
- Smoke checks that execute documented example commands and drift detection between docs examples and command interfaces
- Validation checks for required example metadata fields

### Notes/Constraints
- Keep examples concise and copy-pastable
- Avoid undocumented experimental options in v0.1 examples
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
