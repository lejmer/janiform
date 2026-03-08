## Simulator Run Config And Public API

### Description
Define the public simulator API and run configuration contract for seeded tick-event generation. The contract should include seed, horizon, calendar factors, and model parameters.

### Acceptance Criteria
- A typed run configuration exists for seed, start time, horizon, and required model inputs
- Simulator API returns a deterministic event-path result object
- API docs define required and optional fields and expected failure modes
- Invalid run requests fail with structured validation errors

### Unit tests
- Successful simulator initialization and run execution with valid inputs
- Invalid or incomplete run config payloads

### Notes/Constraints
- Keep API model-agnostic enough for reuse by calibration tooling
- Keep public types stable for Python binding consumption
