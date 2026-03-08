## Debug Trace Mode And Diagnostic Artifacts

### Description
Add a debug simulation mode that emits trace artifacts useful for calibration and troubleshooting without changing simulation semantics.

### Acceptance Criteria
- Debug mode can be enabled through run config without modifying core simulation outputs
- Debug tracing can be disabled with minimal runtime overhead in normal runs
- Output schema and metadata are machine-readable and documented
- Trace artifacts include enough event-time transition context to diagnose ordering, carryover, and scale-consistency mismatches

### Unit tests
- Debug mode does not alter primary event output and enabling and disabling debug mode through API config
- Trace artifact schema and required fields
- Trace correctness tests for per-event transition metadata used by coarse-vs-aggregated comparison tooling

### Notes/Constraints
- Keep trace emission separate from RNG and state-transition logic
- Avoid excessive trace payload size for long horizons by supporting sampling or chunking policy
- Include sampling or compression controls that preserve the information needed for scale-consistency triage
