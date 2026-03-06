## Fitted Parameter And Optimizer State Artifacts

### Description
Define and persist artifacts for fitted parameters and optimizer state across calibration stages for persisted runs.

### Acceptance Criteria
- Stage-level parameter artifacts link cleanly into the run manifest.
- Validation detects missing stage outputs and malformed parameter payloads.
- Output schema and metadata are machine-readable and documented.
- Persistence behavior follows explicit defaults: optional overall, ON by default in CLI flows, and OFF by default for imported package usage unless explicitly enabled.
- Artifact requirements are scoped to `execution_mode=disk` runs where persistence is enabled.

### Unit tests
- Optimizer metadata completeness and type checks and round-trip tests for parameter and optimizer artifact persistence.
- Parameter artifact schema validation per stage.

### Notes/Constraints
- Keep stage payload boundaries explicit to support partial reruns.
- Avoid mixing derived diagnostics into parameter artifacts.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
