## Calendar Service Regression Fixtures And Test Matrix

### Description
Create a reusable fixture set and regression matrix for exchange calendar behavior to protect against accidental changes in open and closed classification.

### Acceptance Criteria
- Shared fixtures exist for regular sessions, weekends, holidays, and early closes.
- Regression matrix includes boundary timestamp cases and timezone-sensitive cases.
- Tests run in both Rust and Python layers where applicable.
- Fixture and matrix documentation explains update process and expected outputs.

### Unit tests
- Golden tests that assert expected state for canonical timestamp fixtures.
- Regression tests that fail on boundary-policy drift.

### Notes/Constraints
- Reuse fixtures in downstream seasonality and gap-behavior tests.
- Keep fixture identifiers stable so failures are easy to triage.
