## Calendar Factor Ingestion And Hour Boundary Carryover

### Description
Integrate simulator with hourly calendar factors and enforce deterministic state carryover across hour boundaries and market state transitions.

### Acceptance Criteria
- Simulator consumes interval-aligned `s_h` and relevant calendar state inputs for each simulation hour
- State carryover across hour boundaries preserves full 4D excitation state
- Boundary transitions at interval edges are deterministic and documented
- Missing factor intervals trigger explicit failure or defined fallback behavior
- Factor intervals act as calendar and seasonality controls only and do not impose a fixed event-time grid inside intervals
- Intrainterval event scheduling semantics are documented as event-time updates with sequential Hawkes state transitions

### Unit tests
- Multi-hour simulations verifying boundary carryover consistency and missing factor intervals with expected failure or fallback outcomes
- Factor changes across hour boundaries
- Boundary-adjacent event-time tests verifying that events near hour edges are not snapped to interval boundaries

### Notes/Constraints
- Reuse shared calendar and factor provider interfaces
- Keep boundary semantics consistent with coarse-time engines
- Keep the distinction explicit between interval-aligned factors and non-grid event-time arrivals
