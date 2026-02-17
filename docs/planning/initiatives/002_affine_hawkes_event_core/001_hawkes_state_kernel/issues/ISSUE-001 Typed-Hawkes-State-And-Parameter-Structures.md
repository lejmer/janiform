## ISSUE-001: Typed Hawkes State And Parameter Structures

### Description
Define explicit core-domain structures for the 4D Hawkes excitation state and its kernel parameters, including shared decay and per-component excitation loadings.

### Acceptance Criteria
- Core types exist for `Y++`, `Y+-`, `Y-+`, and `Y--` with explicit field names
- Parameter structures represent shared `beta`, `alpha_ij`, and `rho_ij` without ambiguous indexing
- Construction helpers prevent accidental component reordering
- Type-level docs describe the exact mapping from model equations to fields

### Unit tests
- Construction tests for valid and invalid parameter payloads
- Field mapping against known component ordering

### Notes/Constraints
- Keep types in `crates/core` and free of PyO3 concerns
- Avoid tuple-based state containers in public core interfaces
