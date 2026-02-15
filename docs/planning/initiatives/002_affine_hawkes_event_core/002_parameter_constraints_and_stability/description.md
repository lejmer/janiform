## Project 002: Parameter Constraints And Stability

Define parameter schemas and validators that enforce positivity, structure, and Hawkes stability conditions.

### Context and requirements
- Implement parameter objects and validators for positivity and structural asymmetry constraints.
- Enforce spectral-radius stability for the de-seasonalized Hawkes kernel.
- Provide clear failure messages for invalid parameter sets used in calibration or simulation.

### Definition of Done
- Validation layer enforces baseline, decay, and excitation constraints including `rho(Alpha / beta) < 1`.
- Structured base-plus-delta parameterization is supported for both `alpha` and `rho` matrices.
- Unit tests cover valid edge cases and expected invalid configurations.

### Agent implementation notes
- Keep constraints near parameter construction to fail fast at model setup time.
- Expose validator outputs in a form usable by optimization routines.

### Initiative alignment
- Satisfies Initiative 002 signal for explicit parameter validation and stability safeguards.
- Improves calibration robustness and prevents explosive simulation regimes.
