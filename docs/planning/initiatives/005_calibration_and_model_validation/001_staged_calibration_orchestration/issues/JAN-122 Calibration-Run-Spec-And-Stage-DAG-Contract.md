## Calibration Run Spec And Stage DAG Contract

### Description
Define the calibration run specification and stage dependency graph contract for end-to-end orchestration across seasonality, Hawkes, and return-component fitting.

### Acceptance Criteria
- A typed run spec exists for data inputs, stage selections, seeds, and optimizer settings
- Stage dependency graph is explicit and validated before execution
- Unsupported or cyclic stage dependencies fail with clear errors
- Run spec and stage graph are serialized for reproducibility
- Solver settings used for each run are persisted in artifacts for auditability

### Unit tests
- Valid stage graph construction and validation and serialization round-trip tests for run specs
- Invalid dependency and cycle detection behavior

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Keep stage graph model independent from optimizer implementation details
- Avoid implicit stage ordering rules
