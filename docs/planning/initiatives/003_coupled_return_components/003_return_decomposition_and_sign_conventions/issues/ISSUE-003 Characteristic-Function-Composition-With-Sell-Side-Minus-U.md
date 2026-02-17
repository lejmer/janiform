## ISSUE-003: Characteristic Function Composition With Sell Side Minus U

### Description
Implement pricing-side characteristic-function composition that applies sell-side exponent with `-u` argument and combines with gap characteristic factor.

### Acceptance Criteria
- CF assembler computes `exp(A_plus * psi_plus(u) + A_minus * psi_minus(-u)) * phi_gap(u)`
- Sell-side `-u` convention is explicit and documented in code
- CF assembly path is shared across pricing and coarse-time transform consumers
- Assembly detects and reports invalid input domain conditions

### Unit tests
- Controlled argument values verifying sell-side `-u` behavior and full CF assembly with and without gap factor
- Regression tests that fail on sign-convention inversion

### Notes/Constraints
- Keep sign handling centralized in one helper module
- Do not duplicate CF wiring logic in multiple engines
