## ISSUE-007: Regression Diagnostics And Drift Attribution Reports

### Description
Implement diagnostics reports that attribute regressions to numerical drift, performance drift, or scenario changes and provide actionable triage details.

### Acceptance Criteria
- Reports classify failures by component and drift category.
- Diagnostics include scenario IDs, threshold IDs, and metric deltas.
- Report format is machine-readable and human-readable.
- Reports support direct comparison with prior baseline runs.

### Unit tests
- Tests for diagnostic report schema and required fields.
- Tests for deterministic drift-category classification rules.
- Tests for report comparison outputs on controlled regression inputs.

### Notes/Constraints
- Keep classification rules explicit and auditable.
- Prioritize concise diagnostics that speed triage.
