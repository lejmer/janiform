## Transform Equation Specification And Data Model

### Description
Define the canonical equation specification and data model for the exponential affine transform over one interval, including `C(t)`, `D(t)`, inputs, and outputs.

### Acceptance Criteria
- A typed model exists for solver inputs including `u_plus`, `u_minus`, `z_plus`, `z_minus`, `s_h`, `nu_h`, explicit interval length `Delta`, and initial state
- A typed model exists for outputs including `C(Delta)`, `D(Delta)`, and transform value `F`
- Equation mapping from design document to implementation fields is documented
- Input validation fails with clear errors for missing or malformed fields

### Unit tests
- Valid model construction from reference payloads and serialization tests for solver input and output models
- Invalid input payload handling and error messages

### Notes/Constraints
- Keep model definitions in `crates/janiform` and reusable by pricing and risk engines
- Avoid anonymous tuple inputs in public solver interfaces
