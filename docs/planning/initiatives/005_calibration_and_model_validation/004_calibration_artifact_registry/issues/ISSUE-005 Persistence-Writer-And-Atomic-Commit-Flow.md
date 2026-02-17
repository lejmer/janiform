## ISSUE-005: Persistence Writer And Atomic Commit Flow

### Description
Implement artifact persistence writer with atomic commit behavior so calibration outputs are never left in partial states.

### Acceptance Criteria
- Writer persists all artifact categories to structured storage layout
- Commit flow is atomic at run level with clear success or failure semantics
- Partial writes are detected and rolled back or quarantined
- Persistence configuration supports local and CI execution contexts

### Unit tests
- Successful full-run artifact persistence and interrupted-write handling and recovery behavior
- Deterministic file and path layout generation

### Notes/Constraints
- Keep writer behavior deterministic across platforms
- Avoid silent overwrites of existing run artifacts
