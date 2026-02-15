## ISSUE-004: Return Component Stage Execution Module

### Description
Implement return-component calibration stage for NIG and coupled gap-related parameters using outputs from prior stages and configured objective terms.

### Acceptance Criteria
- Stage consumes calibrated activity-side inputs and fits return-component parameters.
- Stage supports configured objective components and weighting policy.
- Stage output includes parameter estimates plus objective diagnostics.
- Stage execution supports standalone and pipeline-integrated modes.

### Unit tests
- Tests for successful return-stage execution on reference fixtures.
- Tests for missing dependency artifacts and expected failures.
- Tests for deterministic stage outputs under fixed settings.

### Notes/Constraints
- Reuse shared NIG and coupling modules from Initiative 003.
- Keep objective-term configuration explicit.
