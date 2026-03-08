## CLI Smoke Test Matrix And Regression Coverage

### Description
Build smoke-test matrix for representative CLI workflows and add regression checks for command contracts, outputs, and exit codes, with file-driven calibration as a first-class scenario.

### Acceptance Criteria
- Smoke suite covers one representative path per primary command family.
- Regression checks validate structured output, exit codes, and key argument paths.
- Calibration smoke and regression checks cover default `execution_mode=disk` and explicit override to `memory`.
- Test matrix is bounded for reliable CI execution.
- Failure reports include command, args, and scenario identifiers.
- Smoke coverage includes at least one realistic calibration run driven from `csv` or `parquet` fixture data plus session and known-news inputs.
- Regression coverage includes one advanced staged-execution-input path so the internal mode remains intentionally supported.

### Unit tests
- Smoke tests for simulation, calibration, and pricing commands.
- Output-schema and exit-code stability.
- Negative tests for common invalid invocation patterns.
- Calibration command tests for mode-default, mode-override, and mode-incompatible option behavior.
- Calibration smoke tests for file-format handling, column mapping, and basic provenance reporting on user-facing workflows.

### Notes/Constraints
- Keep smoke suite fast to support merge-time checks.
- Separate deep integration tests from smoke-level coverage.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
