## ISSUE-003: Shared Random Control And Variance Reduction Policy

### Description
Implement shared-random control for paired experiments to reduce comparison noise and improve sensitivity to true coarse-versus-fine divergence.

### Acceptance Criteria
- Paired runs support controlled seed derivation for coarse and fine simulations.
- Variance-reduction policy is explicit and configurable.
- Repeated paired runs with identical settings produce reproducible comparison inputs.
- Metadata records random-control settings per run.

### Unit tests
- Tests for deterministic seed-derivation behavior.
- Tests for reproducibility of paired random streams.
- Tests verifying metadata capture of random-control policy.

### Notes/Constraints
- Reuse RNG abstractions used elsewhere in the simulation stack.
- Avoid hidden randomness in comparison pipeline stages.
