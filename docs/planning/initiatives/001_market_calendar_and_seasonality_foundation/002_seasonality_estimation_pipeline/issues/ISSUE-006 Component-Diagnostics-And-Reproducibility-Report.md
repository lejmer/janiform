## ISSUE-006: Component Diagnostics And Reproducibility Report

### Description
Implement diagnostics and reporting for each seasonal component and overall fit quality, with reproducibility metadata for reruns and comparisons.

### Acceptance Criteria
- Diagnostic outputs include intraday, day-of-week, day-of-year, and optional overlay component summaries.
- Fit-quality metrics are produced in machine-readable form for downstream validation tooling.
- Reports include reproducibility metadata such as input snapshot, configuration, and estimator version.
- Diagnostics can be generated in batch mode for multiple datasets.

### Unit tests
- Tests for metric generation on controlled fixtures.
- Tests for report schema validation and required metadata fields.
- Regression tests for deterministic report output given fixed inputs.

### Notes/Constraints
- Keep reporting format stable for calibration artifact persistence.
- Ensure diagnostics are consumable by both Python and Rust workflows.
