## ISSUE-006: Checkpointing And Resume Orchestration

### Description
Implement checkpoint and resume support so staged calibration can restart from intermediate artifacts after interruptions or targeted reruns.

### Acceptance Criteria
- Checkpoints are emitted after each completed stage with deterministic identifiers
- Resume mode can continue from the last successful stage or a selected stage
- Resume validation checks artifact compatibility before execution
- Resume behavior is documented for success and failure scenarios

### Unit tests
- Checkpoint creation and metadata integrity and resume-from-stage and resume-from-last behavior
- Incompatible-checkpoint failure handling

### Notes/Constraints
- Reuse artifact registry schema from Initiative 005 project planning
- Keep checkpoint writes atomic to avoid partial-state corruption
