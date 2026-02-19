## Canonical Transition API And Engine Adoption

### Description
Create one canonical state-transition API that composes drift, endogenous jumps, and exogenous jumps, then integrate it across simulator, calibration, and transform entrypoints.

### Acceptance Criteria
- A single core API exposes deterministic transition primitives and composition helpers
- Fine-scale simulation uses this API for state evolution
- Coarse-time transform and calibration paths use the same API without local reimplementation
- Integration points are documented with call-order expectations

### Unit tests
- Tests verifying equivalent outputs between composed transitions and reference sequences
- Integration tests confirming all target engines call the shared transition API

### Notes/Constraints
- Keep API minimal and explicit to reduce misuse risk
- Do not introduce duplicate transition paths in downstream modules
