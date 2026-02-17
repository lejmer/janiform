## ISSUE-002: Structured Alpha Parameterization Validation

### Description
Implement structured base-plus-delta parameterization for endogenous excitation `alpha` and validate model-implied asymmetry constraints.

### Acceptance Criteria
- Parameterization supports `a_self`, `a_cross`, `delta_self`, and `delta_cross` inputs
- Derived matrix entries satisfy `alpha_-- >= alpha_++` and `alpha_-+ >= alpha_+-`
- Invalid structured inputs produce clear validation errors
- Derived matrix output is explicit and reusable by downstream engines

### Unit tests
- Valid structured inputs and derived matrix correctness
- Invalid delta and base values that violate constraints and edge tests for boundary cases where deltas are zero

### Notes/Constraints
- Keep derivation and validation logic centralized in one module
- Preserve deterministic ordering for matrix entry output
