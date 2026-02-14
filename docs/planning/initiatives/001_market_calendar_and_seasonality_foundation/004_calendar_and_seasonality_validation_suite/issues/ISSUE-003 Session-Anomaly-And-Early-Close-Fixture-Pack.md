## ISSUE-003: Session Anomaly And Early Close Fixture Pack

### Description
Add fixtures for known session anomalies including early closes, delayed opens, and boundary anomalies so resolver behavior is protected by regression coverage.

### Acceptance Criteria
- Fixture pack includes early-close and delayed-open examples with expected boundary outputs.
- Anomaly fixtures specify deterministic precedence behavior for overlapping rules.
- Fixtures include timezone-aware timestamps at transition boundaries.
- Fixture documentation describes rationale for each anomaly case.

### Unit tests
- Regression tests for early-close transition behavior at boundary timestamps.
- Regression tests for delayed-open windows.
- Conflict-case tests for rule precedence using anomaly fixtures.

### Notes/Constraints
- Keep anomaly fixture semantics consistent with project boundary policy.
- Reuse common timestamp utilities used by other validation suites.
