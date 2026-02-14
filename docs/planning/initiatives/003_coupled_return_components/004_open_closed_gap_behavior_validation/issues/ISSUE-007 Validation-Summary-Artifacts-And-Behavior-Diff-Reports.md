## ISSUE-007: Validation Summary Artifacts And Behavior Diff Reports

### Description
Generate summary artifacts and behavior-diff reports that make open and closed outcome differences easy to inspect and audit.

### Acceptance Criteria
- Validation run outputs machine-readable summaries with scenario-level results.
- Reports show separated activity and gap contributions for each scenario.
- Diff views highlight regressions versus baseline expected outputs.
- Artifact schema and update workflow are documented.

### Unit tests
- Tests for summary artifact schema and required fields.
- Tests for diff-report generation on controlled baseline changes.
- Regression tests ensuring report determinism for fixed inputs.

### Notes/Constraints
- Keep artifact format stable for downstream tooling.
- Avoid environment-specific metadata in report payloads.
