## ISSUE-002: Esscher Tilt Risk Neutral Transformation Module

### Description
Implement Esscher-tilt transformation utilities for Levy components under the risk-neutral setup and enforce martingale-condition wiring for pricing runs.

### Acceptance Criteria
- Module computes transformed exponent terms using `psi_Q(u) = psi_P(u + i * theta) - psi_P(i * theta)`
- Transformation inputs and outputs are explicit and validated
- Martingale-condition checks are exposed for pricing configuration validation
- Transformation logic is reusable by pricing and calibration workflows

### Unit tests
- Transformed exponent values on controlled fixtures and martingale-condition validation behavior
- Invalid transform-parameter handling

### Notes/Constraints
- Keep Hawkes dynamics unchanged under `Q` in v0.1 implementation
- Isolate transform math from Fourier integration code
