## ISSUE-005: Interval Parametric Grid Support And Hourly Compatibility

### Description
Support configurable interval grids for risk generation, including sub-hour intervals when compatible factors are provided, while preserving hourly compatibility for shared workflows.

### Acceptance Criteria
- Sampler accepts explicit interval length `Delta` and does not hardcode hourly steps.
- Hourly interval mode remains supported for cross-engine comparability.
- Non-hourly intervals validate required factor availability before execution.
- Interval semantics are documented and aligned with oracle-solver interfaces.

### Unit tests
- Tests for hourly default-compatible runs.
- Tests for sub-hour interval runs with compatible factor inputs.
- Tests for unsupported interval requests and clear failure messages.

### Notes/Constraints
- Keep interval handling consistent with Initiative 004 wording updates.
- Avoid implicit resampling behavior without explicit policy.
