## ISSUE-001: Paired Experiment Spec And Run Contract

### Description
Define the run contract and scenario specification for paired coarse-versus-fine consistency experiments across parameter regimes and interval configurations.

### Acceptance Criteria
- A typed run spec exists for paired experiment configuration.
- Spec captures coarse interval, fine interval, horizon, parameter set, and random-seed policy.
- Validation rejects incompatible or incomplete paired-run configurations.
- Spec is serializable for artifact persistence and replay.

### Unit tests
- Tests for valid paired-run spec parsing and validation.
- Tests for invalid interval relationships and missing fields.
- Serialization round-trip tests for run specs.

### Notes/Constraints
- Keep run spec independent from metric-calculation internals.
- Avoid implicit defaults that hide interval mismatches.
