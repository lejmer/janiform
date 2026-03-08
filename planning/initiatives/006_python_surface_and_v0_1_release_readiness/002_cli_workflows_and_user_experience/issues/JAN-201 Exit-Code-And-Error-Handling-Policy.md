## Exit Code And Error Handling Policy

### Description
Define explicit exit-code and error-handling policy so CLI behavior is predictable in automated scripts and CI workflows, especially for file-driven calibration tasks where data-shape and preprocessing failures are common.

### Acceptance Criteria
- Exit-code map is documented for success, validation errors, runtime errors, and internal failures
- Commands return deterministic exit codes for equivalent failure classes
- Error output includes actionable context without leaking internals
- Error behavior is consistent across all primary commands
- Calibration error reporting distinguishes user-correctable file and schema problems from runtime calibration failures.
- Error messages for file-driven calibration name the failing file, column or field, and normalization step when applicable.

### Unit tests
- Expected exit codes in nominal and failure scenarios, error-message formatting and required context fields, and consistent failure classification across commands
- Calibration-specific failures for missing files, unsupported file formats, ambiguous timestamps, missing required columns, invalid session definitions, and inconsistent known-news inputs

### Notes/Constraints
- Keep exit-code map small and stable
- Avoid command-specific one-off error semantics
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
