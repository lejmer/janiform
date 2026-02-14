## ISSUE-005: Calendar Factor Ingestion And Hour Boundary Carryover

### Description
Integrate simulator with hourly calendar factors and enforce deterministic state carryover across hour boundaries and market state transitions.

### Acceptance Criteria
- Simulator consumes interval-aligned `s_h` and relevant calendar state inputs for each simulation hour.
- State carryover across hour boundaries preserves full 4D excitation state.
- Boundary transitions at interval edges are deterministic and documented.
- Missing factor intervals trigger explicit failure or defined fallback behavior.

### Unit tests
- Tests for multi-hour simulations verifying boundary carryover consistency.
- Tests for factor changes across hour boundaries.
- Tests for missing factor intervals with expected failure or fallback outcomes.

### Notes/Constraints
- Reuse shared calendar and factor provider interfaces.
- Keep boundary semantics consistent with coarse-time engines.
