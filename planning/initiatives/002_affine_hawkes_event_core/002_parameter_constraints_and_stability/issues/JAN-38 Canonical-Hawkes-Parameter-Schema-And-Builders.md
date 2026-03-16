## Canonical Hawkes Parameter Schema And Builders

### Description
Define a canonical schema for Hawkes kernel parameters and construction builders used by simulation, calibration, and transform workflows.

### Acceptance Criteria
- A single parameter object represents `lambda_bar`, shared `beta`, `alpha` terms, and `rho` terms
- Builder functions support safe construction from typed inputs
- Required fields and domain expectations are documented in code and planning docs
- Parameter object can be serialized for calibration artifact persistence

### Unit tests
- Successful construction from valid typed input and round-trip serialization tests for canonical parameter payloads
- Missing or malformed field handling

### Notes/Constraints
- Keep schema in `crates/janiform` with no dependency on Python boundary types
- Avoid duplicate parameter structs across modules
