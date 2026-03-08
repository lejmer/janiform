## Deterministic Regression CLI Command

### Description
Add a thin CLI command for the deterministic regression harness that already exists in the public Python validation surface, so users can run calendar and hourly-factor regression checks without writing a Python wrapper script.

The implementation should reuse the existing `janiform.validation.run_deterministic_regression_harness` entry point and current CLI output and error conventions. The goal is not to redesign the harness contract, only to make the existing workflow scriptable from the command line.

### Acceptance Criteria
- The CLI exposes a deterministic regression command under the existing developer regression namespace.
- The command accepts calendar-registry input and regression-case inputs in a file-driven form consistent with current CLI conventions.
- The command emits the deterministic regression harness report in machine-readable form and offers a human-readable mode when useful.
- Failures from malformed input, contract validation, and workflow execution are classified through the established CLI error shape and exit codes.
- CLI help and command reference docs include usage examples for the new command.

### Unit tests
- Parser tests for the new command path and required arguments.
- CLI contract tests for successful deterministic regression execution with fixture-backed inputs.
- Failure-path tests for malformed calendar cases, malformed seasonality cases, and missing required files.

### Notes/Constraints
- Reuse the existing public Python harness instead of adding new Rust or PyO3 entry points.
- Keep the command adjacent to the existing open-closed-gap leakage regression workflow to preserve discoverability.
