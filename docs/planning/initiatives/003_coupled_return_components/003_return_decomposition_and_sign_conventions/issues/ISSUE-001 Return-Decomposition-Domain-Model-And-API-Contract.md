## ISSUE-001: Return Decomposition Domain Model And API Contract

### Description
Define the canonical domain model and API contract for return decomposition `X = L_plus(A_plus) - L_minus(A_minus) + G`, with explicit fields for each component.

### Acceptance Criteria
- A shared return-decomposition model exists in core with explicit `A_plus`, `A_minus`, and `gap` fields.
- API contract documents component meanings and units.
- Constructor and validation logic prevents missing or ambiguous component assignment.
- Contract is reusable by both pricing and simulation modules.

### Unit tests
- Tests for valid contract construction and serialization.
- Tests for invalid or incomplete component payloads.
- Tests ensuring explicit field mapping prevents accidental swaps.

### Notes/Constraints
- Keep model independent from engine-specific internals.
- Avoid tuple-based anonymous component passing in public interfaces.
