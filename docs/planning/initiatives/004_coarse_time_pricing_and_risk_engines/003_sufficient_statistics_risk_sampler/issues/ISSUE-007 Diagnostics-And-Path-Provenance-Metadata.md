## ISSUE-007: Diagnostics And Path Provenance Metadata

### Description
Add diagnostics and provenance metadata for risk paths to support debugging, reproducibility, and downstream analysis.

### Acceptance Criteria
- Each run can emit metadata including seed, interval settings, parameter snapshot, and solver status summaries.
- Diagnostic schema is machine-readable and documented.
- Metadata is available in both success and failure paths.
- Provenance outputs integrate with calibration and validation artifact workflows.

### Unit tests
- Tests for diagnostics payload schema and required fields.
- Tests for deterministic metadata values on repeated runs.
- Failure-path tests ensuring diagnostic context is emitted.

### Notes/Constraints
- Keep diagnostics concise and useful for triage.
- Avoid environment-specific identifiers in metadata payloads.
