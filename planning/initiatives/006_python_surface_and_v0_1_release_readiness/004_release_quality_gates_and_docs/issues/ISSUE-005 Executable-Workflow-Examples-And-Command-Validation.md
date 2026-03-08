## ISSUE-005: Executable Workflow Examples And Command Validation

### Description
Publish practical v0.1 workflow examples and validate that documented commands remain executable against the current repository.

### Acceptance Criteria
- Examples cover simulation, calibration, and pricing day-one workflows
- Example commands are validated in automated checks or scripted smoke runs
- Output expectations are documented at a high level
- Example paths are kept in sync with CLI and API contracts
- Calibration examples include at least one `csv`-based workflow and one `parquet`-based workflow, or a documented reason why one file type is intentionally deferred
- Advanced staged execution examples are treated as expert material rather than the main onboarding path

### Unit tests
- Smoke checks that execute documented example commands and drift detection between docs examples and command interfaces
- Validation checks for required example metadata fields
- Validation that documented calibration examples reference user-supplied market-data files, session configuration, and known-news inputs

### Notes/Constraints
- Keep examples concise and copy-pastable
- Avoid undocumented experimental options in v0.1 examples
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
