## Return Component Stage Execution Module

### Description
Implement return-component calibration stage for NIG and coupled gap-related parameters using outputs from prior stages and configured objective terms.

### Acceptance Criteria
- Stage consumes calibrated activity-side inputs and fits return-component parameters
- Stage supports configured objective components and weighting policy
- Stage output includes parameter estimates plus objective diagnostics
- Stage execution supports standalone and pipeline-integrated modes

### Unit tests
- Successful return-stage execution on reference fixtures
- Missing dependency artifacts and expected failures
- Deterministic stage outputs under fixed settings

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Reuse shared NIG and coupling modules from Initiative 003
- Keep objective-term configuration explicit
