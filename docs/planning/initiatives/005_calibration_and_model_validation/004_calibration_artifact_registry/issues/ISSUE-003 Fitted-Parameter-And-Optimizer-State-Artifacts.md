## ISSUE-003: Fitted Parameter And Optimizer State Artifacts

### Description
Define and persist artifacts for fitted parameters and optimizer state across calibration stages.

### Acceptance Criteria
- Stage-level parameter artifacts link cleanly into the run manifest
- Validation detects missing stage outputs and malformed parameter payloads
- Output schema and metadata are machine-readable and documented
- Persistence behavior follows explicit defaults: optional overall, ON by default in CLI flows, and OFF by default for imported package usage unless explicitly enabled

### Unit tests
- Optimizer metadata completeness and type checks and round-trip tests for parameter and optimizer artifact persistence
- Parameter artifact schema validation per stage

### Notes/Constraints
- Keep stage payload boundaries explicit to support partial reruns
- Avoid mixing derived diagnostics into parameter artifacts
