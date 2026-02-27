## ISSUE-007: Report Generation Tables Plots And Artifact Export

### Description
Generate diagnostics reports as in-memory-first result objects plus optional machine-readable artifact export and human-readable tables or plots for decision workflows.

### Acceptance Criteria
- Report generator produces summary tables, plot-ready outputs, and structured in-memory report objects.
- Report outputs can be generated for single and batched calibration runs.
- Export to persisted artifacts is optional and enabled when `execution_mode=disk`.
- Output schema and metadata are machine-readable and documented.

### Unit tests
- Table-summary generation on reference fixtures.
- In-memory report schema validation and required metadata checks.
- Deterministic export file structure and naming for `disk` mode.

### Notes/Constraints
- Keep plotting layer optional so headless workflows are supported.
- Avoid hardcoded paths in export configuration.
