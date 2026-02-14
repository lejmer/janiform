## ISSUE-004: Buy Sell Shared Plus Shift Driver Configuration

### Description
Implement buy and sell driver configuration using shared core NIG parameters plus one sell-side skew shift parameter.

### Acceptance Criteria
- Buy driver uses `(alpha, beta_0, delta, mu)`.
- Sell driver uses `(alpha, beta_0 + delta_beta, delta, mu)` with explicit domain checks.
- Configuration API exposes both drivers from one shared parameter source.
- Invalid skew-shift combinations fail validation with clear errors.

### Unit tests
- Tests for correct buy and sell parameter derivation from shared configuration.
- Tests for invalid derived sell-side domain conditions.
- Tests for deterministic configuration output structure.

### Notes/Constraints
- Keep shared-plus-shift mapping centralized to avoid drift.
- Do not create separate unrelated buy and sell parameter models.
