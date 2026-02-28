## ISSUE-005: Persistence Writer And Atomic Commit Flow

### Description
Implement `disk`-mode artifact persistence writer with atomic commit behavior so persisted calibration outputs are never left in partial states.

### Acceptance Criteria
- Writer persists all artifact categories to structured storage layout for `disk`-mode persisted runs.
- Commit flow is atomic at run level with clear success or failure semantics.
- Partial writes are detected and rolled back or quarantined.
- Persistence configuration supports local and CI execution contexts.
- Persistence controls are explicit and documented: persisted artifacts are optional, default ON for CLI workflows, and default OFF for imported package usage unless explicitly enabled.

### Unit tests
- Successful full-run artifact persistence and interrupted-write handling and recovery behavior.
- Deterministic file and path layout generation.

### Notes/Constraints
- Keep writer behavior deterministic across platforms.
- Avoid silent overwrites of existing run artifacts.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
