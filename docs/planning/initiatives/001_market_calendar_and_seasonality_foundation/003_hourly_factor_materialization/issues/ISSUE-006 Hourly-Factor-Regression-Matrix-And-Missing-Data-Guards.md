## ISSUE-006: Hourly Factor Regression Matrix And Missing Data Guards

### Description
Build regression fixtures and a validation matrix for hourly factor outputs, including missing-data guard behavior and boundary scenarios.

### Acceptance Criteria
- Regression fixtures cover normal sessions, weekends, holidays, and daylight saving transitions.
- Missing-data guard behavior is deterministic and produces defined fallback or failure outcomes.
- Validation matrix includes expected `s_h` and `nu_h` outputs for canonical cases.
- Fixture and validation documentation describes update workflow and acceptance thresholds.

### Unit tests
- Golden tests for canonical hourly output fixtures.
- Regression tests for boundary transitions and missing-data cases.
- Tests verifying deterministic fallback policy for sparse source inputs.

### Notes/Constraints
- Reuse shared fixtures from calendar and seasonality projects where possible.
- Keep fixture identifiers stable for long-term regression tracking.
