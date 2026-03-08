## Exogenous News Loading Jump Operator

### Description
Implement exogenous jump updates that load news-shock severity into Hawkes state using `rho_ij` coefficients and marked severity `U`.

### Acceptance Criteria
- News jump operator applies `rho_ij * U` to each corresponding state component
- Operator accepts non-negative severity marks and enforces deterministic behavior for `U = 0`
- Exogenous updates compose correctly with drift and buy or sell event jumps
- Operator interface supports reuse in both simulation and transform workflows

### Unit tests
- Composition tests combining exogenous and endogenous jumps
- Validation tests for invalid negative severity marks
- Expected component increments across representative `U` values

### Notes/Constraints
- Keep severity-mark validation explicit and local to boundary inputs
- Avoid coupling to a specific news-arrival process implementation
