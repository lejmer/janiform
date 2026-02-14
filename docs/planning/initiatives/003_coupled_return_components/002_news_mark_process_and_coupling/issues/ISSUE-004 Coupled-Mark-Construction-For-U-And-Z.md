## ISSUE-004: Coupled Mark Construction For U And Z

### Description
Implement coupled mark construction that derives gap jump marks `Z = sigma_G * sqrt(U) * epsilon` from shared severity and base-shock marks.

### Acceptance Criteria
- Mark-construction path consumes shared `U` and `epsilon` draws to produce `Z` deterministically.
- Construction supports configurable `sigma_G` with domain validation.
- Coupling logic is centralized so no duplicated mark-construction paths exist.
- Construction metadata supports diagnostics and replay.

### Unit tests
- Tests for deterministic `Z` construction from controlled `U` and `epsilon` inputs.
- Validation tests for invalid `sigma_G` values.
- Regression tests ensuring shared `U` usage across coupled consumers.

### Notes/Constraints
- Keep coupling logic composable for future alternative mark families.
- Do not recompute `U` independently per consumer.
