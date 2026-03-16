## Bench Test Support And Ownership Boundary Enforcement

### Description
Move bench-only and test-only support to the right locations and add enforcement around crate ownership boundaries so the internal split stays healthy after the first extraction work lands.

### Implementation details
- Audit test-only and bench-only helpers currently living in production crate code.
- Move benchmark support that exists only for local or CI benchmarking out of production crates where practical.
- Define where shared integration-test support belongs after the split:
  - crate-local tests
  - workspace integration tests
  - bench-only support
  - governance-only fixture support
- Add ownership and boundary enforcement checks, such as:
  - cargo metadata or workspace policy checks
  - compile-fail or lint-like dependency rules
  - contributor guidance for where new code should land
- Update documentation so contributors understand the new internal ownership map and do not reintroduce monolithic growth by convenience.

### Acceptance Criteria
- Benchmark-only and test-only helpers are no longer defaulting into production crate surfaces without justification.
- The workspace has explicit checks or policies that help keep dependency directions and ownership boundaries intact.
- Contributor documentation points new code toward the right internal crate boundaries.
- The refactor does not regress into a new umbrella monolith spread across several crates in name only.

### Unit tests
- Boundary-policy tests or tooling smoke tests asserting forbidden dependencies are not introduced.
- Benchmark smoke checks confirming bench targets still work after support relocation.
- Documentation tests ensuring contributor guidance references the intended internal ownership map.

### Notes/Constraints
- This issue should follow the initial crate extraction work closely so the new architecture is defended early.
- Avoid adding broad utility crates that become a new dumping ground for unrelated helpers.
