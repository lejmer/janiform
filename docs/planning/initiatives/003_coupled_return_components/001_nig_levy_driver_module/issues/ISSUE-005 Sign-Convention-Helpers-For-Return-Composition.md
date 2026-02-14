## ISSUE-005: Sign Convention Helpers For Return Composition

### Description
Provide helper functions for correct buy and sell characteristic-function contribution wiring, including sell-side `psi_minus(-u)` usage in return composition.

### Acceptance Criteria
- Helper API exposes buy-side `psi_plus(u)` and sell-side `psi_minus(-u)` composition utilities.
- Sign conventions are explicit and documented with model equation references.
- Helpers are reused by pricing and coarse-time transform integration code.
- Regression checks fail on sign-convention regressions.

### Unit tests
- Tests validating helper outputs for controlled argument values.
- Tests detecting incorrect sell-side sign handling.
- Integration-oriented tests for combined buy and sell contribution assembly.

### Notes/Constraints
- Keep sign-convention logic in one module.
- Avoid embedding sign handling separately in each engine.
