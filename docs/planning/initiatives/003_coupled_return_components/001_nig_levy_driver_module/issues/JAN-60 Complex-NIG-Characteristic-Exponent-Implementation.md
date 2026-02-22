## Complex NIG Characteristic Exponent Implementation

### Description
Implement the NIG characteristic exponent `psi_NIG(u)` for complex arguments with numerically stable evaluation for pricing and transform workflows.

### Acceptance Criteria
- Exponent implementation supports complex `u` values required by characteristic-function pricing
- Formula implementation is documented with explicit parameter mapping
- Numerical behavior is stable for representative pricing-grid argument ranges
- API returns deterministic outputs for identical inputs

### Unit tests
- Exponent values against reference fixtures at selected complex arguments and conjugate-symmetry properties expected for characteristic exponents
- Determinism tests across repeated calls

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Keep branch handling for complex square-root operations explicit
- Avoid ad hoc approximations unless they are documented and tested
