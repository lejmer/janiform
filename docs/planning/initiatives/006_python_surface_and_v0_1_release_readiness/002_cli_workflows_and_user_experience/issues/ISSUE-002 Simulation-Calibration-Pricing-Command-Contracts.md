## ISSUE-002: Simulation Calibration Pricing Command Contracts

### Description
Define and implement command-level input and output contracts for the core workflow commands.

### Acceptance Criteria
- Each workflow command has documented required and optional arguments.
- Command contracts map cleanly to stable Python API request models.
- Invalid argument combinations fail with deterministic validation errors.
- Command outputs include required fields for downstream automation.

### Unit tests
- Tests for valid command invocations per workflow.
- Tests for invalid argument combinations and expected validation messages.
- Tests for argument-to-API request mapping correctness.

### Notes/Constraints
- Keep workflow contracts consistent across commands.
- Reuse shared validation utilities where possible.
