## Pricing Characteristic Function Assembly Refactor For M And G

### Description
Refactor pricing characteristic-function composition so direct open-window news jumps and closed-window gap jumps are both represented in total-return assembly.

### Implementation details
- Refactor `crates/core/src/pricing.rs` total-return CF assembly contracts to compose trading-time, open-jump, and closed-gap factors explicitly.
- Update factor-composition structs and error taxonomy so each factor is validated and surfaced independently.
- Refactor oracle-output composition helpers so transformed outputs can be combined with component-aware news-jump factors consistently.
- Update policy identifiers and composition metadata to remove gap-only wording.
- Align pricing-side edge-mode behavior with updated return-decomposition component set.
- Update pricing tests and fixtures to cover open-jump-only, closed-gap-only, and combined scenarios.

### Acceptance Criteria
- Pricing CF assembly contract supports explicit composition of trading-time, open-jump, and closed-gap factors.
- Oracle-output composition and direct-assembly helpers share one canonical factor-composition policy.
- Pricing diagnostics and edge-mode behavior align with updated return-decomposition semantics.
- Domain-guard and non-finite error handling cover all composed factors.

### Unit tests
- CF assembly correctness against controlled factor fixtures for trading-only, open-jump-only, closed-gap-only, and combined cases.
- Validation failures for non-finite or invalid factor inputs.
- Regression tests for edge-mode behavior with updated component definitions.

### Notes/Constraints
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
- Do not bump schema versions. We don't have users yet. We can do whatever we want, no consequences. Not even an alpha version of the library has been released yet so we don't have to bump schemas during this refactor. All schemas should be called v1 even after the refactor.
- Keep pricing composition terminology synchronized with risk and return-decomposition contracts.
