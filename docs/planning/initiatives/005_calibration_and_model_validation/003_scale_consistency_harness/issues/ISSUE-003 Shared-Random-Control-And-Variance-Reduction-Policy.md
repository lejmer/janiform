## ISSUE-003: Shared Random Control And Variance Reduction Policy

### Description
Implement shared-random control for paired experiments to reduce comparison noise and improve sensitivity to true coarse-versus-fine divergence.

### Acceptance Criteria
- Paired runs support controlled seed derivation for coarse and fine simulations
- Variance-reduction policy is explicit and configurable
- Repeated paired runs with identical settings produce reproducible comparison inputs
- Metadata records random-control settings per run

### Unit tests
- Tests verifying metadata capture of random-control policy
- Deterministic seed-derivation behavior and reproducibility of paired random streams

### Notes/Constraints
- Reuse RNG abstractions used elsewhere in the simulation stack
- Avoid hidden randomness in comparison pipeline stages
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
