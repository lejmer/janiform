## Execution Mode Contract And Mode Aware Run Spec Extensions

### Description
Extend JAN-122 with explicit `execution_mode` behavior and mode-gated run-spec rules so staged calibration can run without mandatory filesystem persistence.

### Acceptance Criteria
- Run spec includes `execution_mode` enum with values `memory` and `disk`.
- Calibration run contract terminology and behavior remove artifact-first assumptions and treat in-memory execution as first-class.
- Validation rules enforce mode-aware requirements and reject incompatible option combinations with deterministic errors.
- Stage handoff contract supports in-memory stage outputs and persisted artifact references.
- Resume contract is explicit: same-process only for `memory`, cross-process from persisted checkpoints for `disk`.
- Defaulting policy is documented and testable by caller surface: CLI defaults to `disk` and Python importable API defaults to `memory`, with explicit override support on both surfaces.

### Unit tests
- Run-spec validation tests for valid and invalid mode-gated combinations.
- Deterministic error classification for incompatible mode and option settings.
- Surface-default tests covering CLI default `disk`, Python API default `memory`, and explicit overrides.

### Notes/Constraints
- JAN-122 remains the base stage-DAG contract.
- JAN-130 defines additive mode-aware extensions and must not break JAN-122 compatibility.
