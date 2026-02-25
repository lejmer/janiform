## Trading And Gap CF Composition Assembly

### Description
Implement assembly of total return characteristic function from oracle-derived trading-time CF and gap contribution for the pricing interval.

### Acceptance Criteria
- Pricing assembly consumes oracle outputs and composes total CF in one canonical path
- Sell-side sign convention is preserved through CF contribution wiring
- Gap factor integration is explicit and documented
- Composition outputs are deterministic for fixed inputs

### Unit tests
- Composition correctness on controlled input scenarios and tests isolating trading-only and gap-only composition paths
- Sign-convention errors in assembled CF

### Notes/Constraints
- Reuse return decomposition helpers from Initiative 003
- Do not duplicate CF assembly logic in separate pricing adapters
