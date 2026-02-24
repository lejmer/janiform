## Validation Summary Artifacts And Behavior Diff Reports

### Description
Generate summary artifacts and behavior-diff reports that make open and closed outcome differences easy to inspect and audit.

### Acceptance Criteria
- Reports show separated activity and gap contributions for each scenario
- Diff views highlight regressions versus baseline expected outputs
- Output schema and metadata are machine-readable and documented

### Unit tests
- Diff-report generation on controlled baseline changes
- Summary artifact schema and required fields
- Regression tests ensuring report determinism for fixed inputs

### Notes/Constraints
- Keep artifact format stable for downstream tooling
- Avoid environment-specific metadata in report payloads
