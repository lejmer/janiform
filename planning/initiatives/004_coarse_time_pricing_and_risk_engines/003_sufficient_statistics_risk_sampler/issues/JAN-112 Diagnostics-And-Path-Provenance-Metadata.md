## Diagnostics And Path Provenance Metadata

### Description
Add diagnostics and provenance metadata for risk paths to support debugging, reproducibility, and downstream analysis.

### Acceptance Criteria
- Provenance outputs integrate with calibration and validation artifact workflows
- Output schema and metadata are machine-readable and documented

### Unit tests
- Diagnostics payload schema and required fields
- Failure-path tests ensuring diagnostic context is emitted
- Deterministic metadata values on repeated runs

### Notes/Constraints
- Keep diagnostics concise and useful for triage
- Avoid environment-specific identifiers in metadata payloads
