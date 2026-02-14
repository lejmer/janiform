## ISSUE-006: Debug Trace Mode And Diagnostic Artifacts

### Description
Add a debug simulation mode that emits trace artifacts useful for calibration and troubleshooting without changing simulation semantics.

### Acceptance Criteria
- Debug mode can be enabled through run config without modifying core simulation outputs.
- Trace artifacts include step-level intensity and state evolution metadata.
- Artifact format is documented and machine-readable.
- Debug tracing can be disabled with minimal runtime overhead in normal runs.

### Unit tests
- Tests confirming debug mode does not alter primary event output.
- Tests for trace artifact schema and required fields.
- Tests for enabling and disabling debug mode through API config.

### Notes/Constraints
- Keep trace emission separate from RNG and state-transition logic.
- Avoid excessive trace payload size for long horizons by supporting sampling or chunking policy.
