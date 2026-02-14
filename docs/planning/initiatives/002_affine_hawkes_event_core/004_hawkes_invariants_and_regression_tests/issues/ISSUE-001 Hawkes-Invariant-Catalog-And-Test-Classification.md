## ISSUE-001: Hawkes Invariant Catalog And Test Classification

### Description
Define the canonical invariant catalog for Hawkes dynamics and classify tests into deterministic equation checks versus statistical-property checks.

### Acceptance Criteria
- A documented invariant catalog exists with unique IDs and short rationale for each invariant.
- Invariants are grouped by category such as constraints, dynamics, boundary behavior, and sampling properties.
- Test classification rules are defined for deterministic versus stochastic checks.
- Catalog is referenced by all regression and invariant test suites in this project.

### Unit tests
- Validation tests for invariant-catalog schema and required fields.
- Tests ensuring every implemented invariant test references a catalog ID.

### Notes/Constraints
- Keep catalog machine-readable for future reporting automation.
- Do not mix deterministic and statistical checks in one invariant definition.
