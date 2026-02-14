## ISSUE-001: Canonical Hawkes Parameter Schema And Builders

### Description
Define a canonical schema for Hawkes kernel parameters and construction builders used by simulation, calibration, and transform workflows.

### Acceptance Criteria
- A single parameter object represents `lambda_bar`, shared `beta`, `alpha` terms, and `rho` terms.
- Builder functions support safe construction from typed inputs.
- Required fields and domain expectations are documented in code and planning docs.
- Parameter object can be serialized for calibration artifact persistence.

### Unit tests
- Tests for successful construction from valid typed input.
- Tests for missing or malformed field handling.
- Round-trip serialization tests for canonical parameter payloads.

### Notes/Constraints
- Keep schema in `crates/core` with no dependency on Python boundary types.
- Avoid duplicate parameter structs across modules.
