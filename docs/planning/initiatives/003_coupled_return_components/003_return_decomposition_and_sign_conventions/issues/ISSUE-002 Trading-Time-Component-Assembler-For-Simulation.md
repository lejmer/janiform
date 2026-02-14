## ISSUE-002: Trading Time Component Assembler For Simulation

### Description
Implement simulation-side assembly of trading-time return contribution from buy and sell drivers conditional on `A_plus` and `A_minus`.

### Acceptance Criteria
- Simulation assembler computes `L_plus(A_plus) - L_minus(A_minus)` with explicit sign handling.
- Assembler supports seeded deterministic execution through shared random interfaces.
- Component-level outputs are available for diagnostics and debugging.
- Assembler interface is consistent with return-decomposition contract.

### Unit tests
- Deterministic seeded tests for simulation assembly outputs.
- Tests for component isolation with buy-only and sell-only scenarios.
- Tests validating sign-correct subtraction of sell contribution.

### Notes/Constraints
- Reuse shared NIG driver module for increment generation.
- Keep assembler logic separate from gap-process handling.
