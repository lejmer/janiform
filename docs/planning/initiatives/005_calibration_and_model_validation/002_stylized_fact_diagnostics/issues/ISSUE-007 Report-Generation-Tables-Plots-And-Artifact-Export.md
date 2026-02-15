## ISSUE-007: Report Generation Tables Plots And Artifact Export

### Description
Generate diagnostics reports as machine-readable artifacts and human-readable tables or plots for decision workflows.

### Acceptance Criteria
- Report generator produces machine-readable diagnostics artifact files.
- Report generator produces summary tables and plot-ready outputs.
- Artifact schema references metric catalog IDs and run metadata.
- Report outputs can be generated for single and batched calibration runs.

### Unit tests
- Tests for artifact schema validation and required metadata.
- Tests for table-summary generation on reference fixtures.
- Tests for deterministic export file structure and naming.

### Notes/Constraints
- Keep plotting layer optional so headless workflows are supported.
- Avoid hardcoded paths in export configuration.
