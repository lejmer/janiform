## ISSUE-001: Timestamp Event Data Contract And Preprocessing

### Description
Define the input contract and preprocessing workflow for seasonality estimation from event timestamps. This includes schema validation, timezone normalization, session filtering, and handling of missing or sparse periods.

### Acceptance Criteria
- A documented input schema exists for timestamp event data required by the estimator.
- Preprocessing pipeline normalizes timestamps to exchange-aware calendar context.
- Invalid records are rejected with actionable validation errors.
- Preprocessing outputs are deterministic for identical inputs.

### Unit tests
- Validation tests for missing required fields and invalid timestamp formats.
- Tests for timezone normalization and session filtering behavior.
- Determinism tests for repeated preprocessing runs.

### Notes/Constraints
- Keep preprocessing reusable by future calibration workflows.
- Do not include model-specific estimation logic in this layer.
