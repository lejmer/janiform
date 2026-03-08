## Return Component Stage Execution Module

### Description
Implement return-component calibration stage for NIG and coupled gap-related parameters using outputs from prior stages and configured objective terms, with mode-aware stage handoffs.

### Acceptance Criteria
- Stage consumes calibrated activity-side inputs and fits return-component parameters.
- Stage supports configured objective components and weighting policy.
- Stage output includes parameter estimates plus objective diagnostics.
- Stage execution supports standalone and pipeline-integrated modes with in-memory and persisted handoff support.

### Unit tests
- Successful return-stage execution on reference fixtures in `memory` and `disk` modes.
- Missing dependency stage outputs and expected failures.
- Deterministic stage outputs under fixed settings across both execution modes.

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Reuse shared NIG and coupling modules from Initiative 003.
- Keep objective-term configuration explicit.
- Follow the combined run-contract guidance in JAN-122 and JAN-130.
