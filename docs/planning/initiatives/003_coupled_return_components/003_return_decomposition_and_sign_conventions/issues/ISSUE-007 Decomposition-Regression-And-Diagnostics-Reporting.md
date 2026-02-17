## ISSUE-007: Decomposition Regression And Diagnostics Reporting

### Description
Build regression and diagnostics reporting for decomposition outputs so sign and component regressions are quickly attributable.

### Acceptance Criteria
- Regression suite covers component-level and total-return decomposition outputs
- Failure reports include component contributions, seed, and scenario ID
- Diagnostics can classify failures as mapping, sign, or component-disable-mode issues
- Reports are machine-readable for CI consumption

### Unit tests
- Representative decomposition scenarios
- Diagnostics-schema tests for required metadata fields
- Failure-classification tests for known injected sign and mapping errors

### Notes/Constraints
- Keep diagnostics concise and actionable
- Avoid environment-specific data in emitted reports
