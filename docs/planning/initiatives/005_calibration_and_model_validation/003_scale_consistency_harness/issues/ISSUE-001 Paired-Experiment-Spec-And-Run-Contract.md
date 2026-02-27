## ISSUE-001: Paired Experiment Spec And Run Contract

### Description
Define the run contract and scenario specification for paired coarse-versus-fine consistency experiments across parameter regimes and interval configurations.

### Acceptance Criteria
- A typed run spec exists for paired experiment configuration.
- Spec captures coarse interval, fine interval, horizon, parameter set, random-seed policy, and `execution_mode`.
- Validation rejects incompatible or incomplete paired-run configurations.
- Spec serialization is supported for replay and transport without requiring filesystem persistence.

### Unit tests
- Serialization round-trip tests for run specs.
- Valid paired-run spec parsing and validation.
- Invalid interval relationships, mode-gated invalid combinations, and missing fields.

### Notes/Constraints
- Keep run spec independent from metric-calculation internals.
- Avoid implicit defaults that hide interval mismatches.
