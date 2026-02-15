## ISSUE-003: Fitted Parameter And Optimizer State Artifacts

### Description
Define and persist artifacts for fitted parameters and optimizer state across calibration stages.

### Acceptance Criteria
- Artifact schema supports seasonality, Hawkes, and return-component parameter outputs.
- Optimizer metadata includes solver settings, convergence status, and iteration summaries.
- Stage-level parameter artifacts link cleanly into the run manifest.
- Validation detects missing stage outputs and malformed parameter payloads.

### Unit tests
- Tests for parameter artifact schema validation per stage.
- Tests for optimizer metadata completeness and type checks.
- Round-trip tests for parameter and optimizer artifact persistence.

### Notes/Constraints
- Keep stage payload boundaries explicit to support partial reruns.
- Avoid mixing derived diagnostics into parameter artifacts.
