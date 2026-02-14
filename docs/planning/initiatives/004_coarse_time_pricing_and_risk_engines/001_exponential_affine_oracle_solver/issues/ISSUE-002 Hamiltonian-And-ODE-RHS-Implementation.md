## ISSUE-002: Hamiltonian And ODE RHS Implementation

### Description
Implement Hamiltonian functions and right-hand-side ODE evaluation for `D_ij` and `C` dynamics based on the formal system in the design notes.

### Acceptance Criteria
- RHS implementation covers all four `D` equations and the `C` equation.
- Hamiltonians `H_plus` and `H_minus` are implemented once and reused across RHS evaluation.
- News-term contribution in `C` equation uses the documented gamma-moment expression.
- RHS evaluation is deterministic for identical inputs.

### Unit tests
- Equation-level tests for RHS output on controlled fixtures.
- Tests for Hamiltonian evaluation against hand-checked numeric examples.
- Regression tests for deterministic RHS behavior.

### Notes/Constraints
- Keep equation logic separate from integration step logic.
- Use explicit component names to reduce index mapping risk.
