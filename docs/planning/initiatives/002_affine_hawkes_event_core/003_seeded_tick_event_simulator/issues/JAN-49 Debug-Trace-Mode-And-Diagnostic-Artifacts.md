## Debug Trace Mode And Diagnostic Artifacts

### Description
Add a debug simulation mode that emits trace artifacts useful for calibration and troubleshooting without changing simulation semantics.

### Acceptance Criteria
- Debug mode can be enabled through run config without modifying core simulation outputs
- Debug tracing can be disabled with minimal runtime overhead in normal runs
- Output schema and metadata are machine-readable and documented

### Unit tests
- Debug mode does not alter primary event output and enabling and disabling debug mode through API config
- Trace artifact schema and required fields

### Notes/Constraints
- Keep trace emission separate from RNG and state-transition logic
- Avoid excessive trace payload size for long horizons by supporting sampling or chunking policy
