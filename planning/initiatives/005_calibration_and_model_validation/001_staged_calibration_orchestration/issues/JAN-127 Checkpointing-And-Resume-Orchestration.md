## Checkpointing And Resume Orchestration

### Description
Implement checkpoint and resume support so staged calibration can restart from intermediate state in mode-aware ways for interruptions or targeted reruns.

### Acceptance Criteria
- Checkpoint state is emitted after each completed stage with deterministic identifiers under the selected execution mode.
- Resume mode can continue from the last successful stage or a selected stage.
- Resume validation checks mode compatibility and checkpoint compatibility before execution.
- Resume behavior is explicit and documented: same-process resume only in `memory` mode and cross-process resume from persisted checkpoints in `disk` mode.

### Unit tests
- Checkpoint metadata integrity and resume-from-stage and resume-from-last behavior in both execution modes.
- Incompatible-checkpoint and mode-incompatible failure handling.

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Reuse artifact registry schema from Initiative 005 project planning for `disk` mode checkpoint persistence.
- Keep checkpoint writes atomic in `disk` mode to avoid partial-state corruption.
- Follow the combined run-contract guidance in JAN-122 and JAN-130.
