## ISSUE-001: Benchmark Scenario Catalog And Workload Profiles

### Description
Define benchmark scenario catalog and workload profiles covering oracle, pricing, and risk sampler usage for representative portfolio conditions.

### Acceptance Criteria
- Scenario catalog includes small, medium, and large workload profiles
- Profiles specify model state, interval settings, portfolio shape, and run length
- Catalog is versioned and machine-readable
- Scenario loader validates profile integrity with clear errors

### Unit tests
- Valid and invalid scenario-profile parsing
- Deterministic profile ordering and loading behavior

### Notes/Constraints
- Keep workload profiles realistic but bounded for repeatable execution
- Reuse shared fixtures where possible
