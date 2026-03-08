## ISSUE-001: Boundary Invariant Spec And Public Type Policy

### Description
Define explicit boundary invariants and public-type policy for the PyO3 layer so only documented Python-facing types are exposed.

### Acceptance Criteria
- Boundary invariant document defines ownership and conversion rules at the Rust-to-Python edge
- Public type policy lists allowed Python-facing types and disallowed internal leaks
- Policy is referenced by pybind module docs and implementation guidelines
- Violations are detectable via review checklist or automated checks where feasible

### Unit tests
- Tests that internal Rust structs are not directly exported
- Tests ensuring exposed Python types match approved public contracts

### Notes/Constraints
- Keep policy minimal and actionable for v0.1
- Align naming with public Python API contracts
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
