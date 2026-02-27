## Seasonality Stage Execution Module

### Description
Implement the seasonality stage execution module that runs deterministic calendar-factor calibration and emits normalized stage outputs for downstream stages in both `memory` and `disk` execution modes.

### Acceptance Criteria
- Stage runner consumes run spec inputs and executes seasonality estimation workflow.
- Output includes normalized seasonality stage payload and metadata needed by later stages.
- Stage output handoff supports in-memory object transfer and persisted artifact reference transfer.
- Stage produces structured success and failure status.
- Stage is callable independently for partial-pipeline runs.

### Unit tests
- Successful seasonality-stage execution on reference fixtures in `memory` and `disk` modes.
- Missing-input and invalid-config failure paths.
- Deterministic output metadata under fixed seed and config across both execution modes.

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Reuse seasonality pipeline interfaces from Initiative 001.
- Keep stage output schema stable for stage handoffs.
- Follow the combined run-contract guidance in JAN-122 and JAN-130.
