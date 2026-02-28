## ISSUE-002: Simulation Calibration Pricing Command Contracts

### Description
Define and implement command-level input and output contracts for the core workflow commands.

### Acceptance Criteria
- Each workflow command has documented required and optional arguments.
- Calibration command contracts include `--execution-mode {disk,memory}` with default `disk`.
- Command contracts map cleanly to Python API request models.
- Command contracts use corrected terminology for known-surprise news flows and open-jump versus closed-gap outputs where surfaced.
- Invalid argument combinations fail with deterministic validation errors.
- Command outputs include required fields for downstream automation.

### Unit tests
- Valid command invocations per workflow and argument-to-API request mapping correctness.
- Invalid argument combinations and expected validation messages.
- Calibration command tests for default mode and explicit override behavior.

### Notes/Constraints
- Keep workflow contracts consistent across commands.
- Reuse shared validation utilities where possible.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
