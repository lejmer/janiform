## ISSUE-001: Calibration Run Spec And Stage DAG Contract

### Description
Define the calibration run specification and stage dependency graph contract for end-to-end orchestration across seasonality, Hawkes, and return-component fitting.

### Acceptance Criteria
- A typed run spec exists for data inputs, stage selections, seeds, and optimizer settings.
- Stage dependency graph is explicit and validated before execution.
- Unsupported or cyclic stage dependencies fail with clear errors.
- Run spec and stage graph are serialized for reproducibility.

### Unit tests
- Tests for valid stage graph construction and validation.
- Tests for invalid dependency and cycle detection behavior.
- Serialization round-trip tests for run specs.

### Notes/Constraints
- Keep stage graph model independent from optimizer implementation details.
- Avoid implicit stage ordering rules.
