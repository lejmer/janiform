## ISSUE-002: Calendar Fixture Reuse And State Resolution Integration

### Description
Integrate calendar fixtures from Initiative 001 into this validation suite so market state resolution is consistent and not duplicated.

### Acceptance Criteria
- Validation scenarios consume shared calendar fixtures for `open` and `closed` classification.
- State resolution uses canonical market-state service, not local mocks.
- Fixture references include at least intraday, overnight, and weekend windows.
- Integration docs describe fixture mapping into scenario inputs.

### Unit tests
- Tests verifying scenario state classification matches calendar fixture expectations.
- Tests for missing fixture references with clear failure messages.

### Notes/Constraints
- Keep fixture dependencies explicit and versioned.
- Do not duplicate calendar logic in this project.
