## ISSUE-002: Simulation Calibration Pricing Command Contracts

### Description
Define and implement command-level input and output contracts for the core workflow commands.

### Acceptance Criteria
- Each workflow command has documented required and optional arguments
- Command contracts map cleanly to stable Python API request models
- Invalid argument combinations fail with deterministic validation errors
- Command outputs include required fields for downstream automation

### Unit tests
- Valid command invocations per workflow and argument-to-API request mapping correctness
- Invalid argument combinations and expected validation messages

### Notes/Constraints
- Keep workflow contracts consistent across commands
- Reuse shared validation utilities where possible
