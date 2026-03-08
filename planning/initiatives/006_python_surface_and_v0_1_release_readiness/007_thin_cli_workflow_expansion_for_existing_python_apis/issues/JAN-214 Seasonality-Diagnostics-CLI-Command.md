## Seasonality Diagnostics CLI Command

### Description
Add a task-oriented CLI command for running the public seasonality estimation and diagnostics workflow from raw timestamp-event input.

The command should orchestrate preprocessing, intraday estimation, day-of-week estimation, day-of-year estimation, deterministic composition, and final diagnostics report generation through the existing public Python APIs. Users should not need to manually chain intermediate steps unless they are debugging.

### Acceptance Criteria
- The CLI exposes a seasonality diagnostics command that starts from event records and a calendar registry.
- The command drives the existing public Python workflow steps and emits the canonical diagnostics report payload.
- Command arguments cover the major estimator and preprocessing settings that are already public and stable.
- Human-readable output summarizes the diagnostic result without hiding the underlying report payload available in JSON mode.
- Docs and examples show the command as the primary CLI path for seasonality diagnostics from raw timestamp-event data.

### Unit tests
- Parser tests for required inputs and representative estimator-setting overrides.
- CLI tests for successful end-to-end diagnostics generation from fixture-backed inputs.
- Failure-path tests for invalid estimator settings, malformed event records, and calendar resolution errors.

### Notes/Constraints
- Prefer one workflow-oriented command over multiple narrowly-scoped estimation commands unless a later need becomes clear.
- Reuse existing public Python models and validation behavior rather than creating a separate CLI-only seasonality contract.
