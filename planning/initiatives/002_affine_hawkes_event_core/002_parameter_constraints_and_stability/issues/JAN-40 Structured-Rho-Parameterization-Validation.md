## Structured Rho Parameterization Validation

### Description
Implement structured base-plus-delta parameterization for exogenous loading `rho` and enforce non-negativity plus asymmetry constraints required by the design.

### Acceptance Criteria
- Parameterization supports `r_self`, `r_cross`, `delta_self_r`, and `delta_cross_r` inputs
- Derived entries satisfy non-negativity and expected asymmetry in self and cross loadings
- Validation includes optional policy check for self-dominant loading relative to cross loading
- Validation output is machine-readable for optimizer feedback loops

### Unit tests
- Valid structured inputs and derived `rho` matrix values and self-dominance policy pass and fail cases
- Invalid inputs that violate non-negativity or ordering constraints

### Notes/Constraints
- Keep self-dominance policy configurable so calibration teams can tune strictness
- Reuse shared validator primitives from other parameter modules
