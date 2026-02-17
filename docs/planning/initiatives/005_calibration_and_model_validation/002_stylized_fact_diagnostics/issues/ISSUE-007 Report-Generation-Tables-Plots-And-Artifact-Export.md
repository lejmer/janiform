## ISSUE-007: Report Generation Tables Plots And Artifact Export

### Description
Generate diagnostics reports as machine-readable artifacts and human-readable tables or plots for decision workflows.

### Acceptance Criteria
- Report generator produces summary tables and plot-ready outputs
- Report outputs can be generated for single and batched calibration runs
- Output schema and metadata are machine-readable and documented

### Unit tests
- Table-summary generation on reference fixtures
- Artifact schema validation and required metadata
- Deterministic export file structure and naming

### Notes/Constraints
- Keep plotting layer optional so headless workflows are supported
- Avoid hardcoded paths in export configuration
