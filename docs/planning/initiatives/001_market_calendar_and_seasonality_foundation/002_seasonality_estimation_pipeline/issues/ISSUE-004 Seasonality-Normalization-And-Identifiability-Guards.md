## ISSUE-004: Seasonality Normalization And Identifiability Guards

### Description
Implement normalization rules for the combined seasonality signal so `s(t)` preserves identifiability with baseline Hawkes intensity. Include safeguards that enforce anchored mean level over the chosen estimation window.

### Acceptance Criteria
- A normalization routine enforces an anchored mean level for `s(t)` over the fit window.
- Identifiability guard checks fail fast when normalization assumptions are violated.
- Normalization metadata is emitted with estimator outputs.
- Behavior is deterministic and documented.

### Unit tests
- Tests verifying anchored mean behavior across representative datasets.
- Negative tests for invalid normalization inputs and degenerate windows.
- Tests ensuring metadata captures normalization choices.

### Notes/Constraints
- Keep normalization logic centralized to avoid inconsistent handling across modules.
- Ensure guard outputs are consumable by calibration diagnostics.
