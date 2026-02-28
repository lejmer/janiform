## ISSUE-007: CLI Smoke Test Matrix And Regression Coverage

### Description
Build smoke-test matrix for representative CLI workflows and add regression checks for command contracts, outputs, and exit codes.

### Acceptance Criteria
- Smoke suite covers one representative path per primary command family.
- Regression checks validate structured output, exit codes, and key argument paths.
- Calibration smoke and regression checks cover default `execution_mode=disk` and explicit override to `memory`.
- Test matrix is bounded for reliable CI execution.
- Failure reports include command, args, and scenario identifiers.

### Unit tests
- Smoke tests for simulation, calibration, and pricing commands.
- Output-schema and exit-code stability.
- Negative tests for common invalid invocation patterns.
- Calibration command tests for mode-default, mode-override, and mode-incompatible option behavior.

### Notes/Constraints
- Keep smoke suite fast to support merge-time checks.
- Separate deep integration tests from smoke-level coverage.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
