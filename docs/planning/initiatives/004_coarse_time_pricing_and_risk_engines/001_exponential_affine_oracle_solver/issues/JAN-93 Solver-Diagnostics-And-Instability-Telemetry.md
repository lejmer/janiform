## Solver Diagnostics And Instability Telemetry

### Description
Add diagnostics and telemetry outputs that help identify numerical instability, guard-trigger events, and integration quality issues.

### Acceptance Criteria
- Solver emits structured diagnostics with step counts, status flags, and guard-trigger context
- Diagnostics are available for both success and failure paths
- Output schema is machine-readable and documented
- Telemetry can be enabled for debug workflows without changing solver semantics

### Unit tests
- Telemetry toggle behavior
- Diagnostic payload schema and required fields
- Expected telemetry output on guarded-failure scenarios

### Notes/Constraints
- Keep diagnostic payload concise and actionable
- Avoid environment-specific paths in telemetry outputs
