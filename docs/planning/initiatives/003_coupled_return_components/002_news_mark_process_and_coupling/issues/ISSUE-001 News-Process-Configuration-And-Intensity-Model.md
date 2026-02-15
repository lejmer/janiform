## ISSUE-001: News Process Configuration And Intensity Model

### Description
Define configuration schema for the shared exogenous news process and implement deterministic intensity model wiring for `nu_news(t)` inputs.

### Acceptance Criteria
- A typed configuration model exists for news intensity and mark distribution parameters.
- Intensity model supports baseline closed-window news intensity policy and deterministic time-dependent inputs used by simulation and pricing paths.
- Configuration validation reports missing or invalid fields with actionable errors.
- Configuration can be serialized for calibration artifact persistence.

### Unit tests
- Validation tests for complete and incomplete configuration payloads.
- Tests for deterministic intensity model output given fixed inputs.
- Round-trip serialization tests for configuration objects.

### Notes/Constraints
- Keep intensity model independent from specific calendar implementation details.
- Keep baseline behavior aligned with closed-window intensity policy from the design specification.
- Avoid duplicated config structs across modules.
