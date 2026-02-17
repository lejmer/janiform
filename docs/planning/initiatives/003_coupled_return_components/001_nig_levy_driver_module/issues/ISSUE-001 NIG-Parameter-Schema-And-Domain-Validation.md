## ISSUE-001: NIG Parameter Schema And Domain Validation

### Description
Define the canonical parameter schema for NIG drivers and implement domain validation for all parameter inputs used by simulation and pricing workflows.

### Acceptance Criteria
- A typed parameter model exists for `alpha`, `beta`, `delta`, and `mu`
- Validation enforces `alpha > abs(beta)` and `delta > 0`
- Validation errors identify failing fields and violated constraints
- Parameter payloads can be serialized for calibration artifacts and replay

### Unit tests
- Valid interior parameter regimes and round-trip serialization tests for parameter payloads
- Invalid boundary and out-of-domain parameter sets

### Notes/Constraints
- Keep parameter validation in one canonical module
- Do not duplicate validation logic across pricing and simulation paths
