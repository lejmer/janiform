## ISSUE-002: Deterministic Randomness Abstraction And Seed Policy

### Description
Implement a deterministic randomness abstraction that provides reproducible streams for event simulation across Rust and Python entrypoints.

### Acceptance Criteria
- Randomness layer supports explicit seeding with documented stream behavior
- Repeated runs with identical seed and inputs produce identical event output
- Seed policy for derived substreams is explicit and deterministic
- Randomness abstraction can be injected for tests and reference fixtures

### Unit tests
- Stable substream derivation behavior
- Repeatability tests for identical seeds and inputs and divergence tests for different seeds

### Notes/Constraints
- Keep RNG usage centralized to avoid hidden nondeterminism
- Ensure seed behavior is independent of logging or debug mode
