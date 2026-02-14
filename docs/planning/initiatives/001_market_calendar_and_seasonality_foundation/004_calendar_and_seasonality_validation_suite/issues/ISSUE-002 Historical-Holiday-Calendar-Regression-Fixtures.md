## ISSUE-002: Historical Holiday Calendar Regression Fixtures

### Description
Create a curated fixture set for historical holiday calendars so open and closed classification can be regression-tested against known exchange closures.

### Acceptance Criteria
- Fixture set includes representative holiday dates across multiple years.
- Each fixture includes expected market state outputs for key intraday timestamps.
- Fixture coverage includes weekdays that are closed due to holidays.
- Fixture provenance is documented for maintainability.

### Unit tests
- Regression tests that assert expected closed classification on holiday dates.
- Tests that assert normal classification on adjacent non-holiday dates.
- Loader tests for holiday fixture integrity and uniqueness.

### Notes/Constraints
- Keep holiday fixtures reusable by other calendar projects.
- Prefer data-driven fixture expansion over hardcoded test logic.
