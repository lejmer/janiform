## ISSUE-007: Pricing Diagnostics And Surface Calibration Hooks

### Description
Expose intermediate pricing diagnostics and optional outputs needed to calibrate model parameters against option surfaces.

### Acceptance Criteria
- Pricing run can emit intermediate diagnostics such as transform and integration quality metrics
- Diagnostic schema is machine-readable and documented
- Optional calibration hooks are available without altering base pricing semantics
- Diagnostics are consumable by calibration workflows

### Unit tests
- Toggled diagnostic output behavior
- Diagnostic payload schema and required fields
- Integration tests for calibration-consumer compatibility

### Notes/Constraints
- Keep diagnostics optional to avoid overhead in default pricing runs
- Avoid exposing internal-only unstable structures through public APIs
