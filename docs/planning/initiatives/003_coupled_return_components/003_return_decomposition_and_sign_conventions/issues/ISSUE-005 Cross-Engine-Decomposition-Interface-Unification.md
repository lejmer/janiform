## ISSUE-005: Cross Engine Decomposition Interface Unification

### Description
Unify decomposition interfaces so simulation and pricing engines consume one shared composition API and component model.

### Acceptance Criteria
- Pricing and simulation import the same decomposition interfaces from core
- Engine-specific adapters are thin and do not reimplement decomposition math
- Integration documentation maps call flow for each engine
- Interface evolution policy is defined for v0.1 stability

### Unit tests
- Adapter tests ensuring no math divergence in boundary layers
- Integration tests confirming both engines use shared decomposition interfaces and tests verifying equivalent component mapping across engine call paths

### Notes/Constraints
- Keep interface minimal but explicit
- Avoid engine-local forks of composition helpers
