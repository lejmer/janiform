## ISSUE-007: Consistency Report And Drift Diagnostics Outputs

### Description
Generate consistency reports and drift diagnostics that summarize deltas, pass-or-fail status, and likely sources of mismatch.

### Acceptance Criteria
- Reports include per-scenario metric deltas and pass-or-fail outcomes.
- Drift diagnostics classify mismatches by returns, volatility, or event-derived categories.
- Report schema is machine-readable and versioned.
- Human-readable summary output is available for review workflows.

### Unit tests
- Tests for report schema and required fields.
- Tests for drift-category classification determinism.
- Regression tests for stable report formatting and ordering.

### Notes/Constraints
- Keep diagnostics concise and actionable.
- Avoid environment-specific fields in report payloads.
