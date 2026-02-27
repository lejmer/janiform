## Orchestration Integration Tests And Failure Playbooks

### Description
Add integration tests and failure playbooks for staged calibration to ensure clear handling of success paths, expected failures, and operator triage workflows across mode-aware orchestration behavior.

### Acceptance Criteria
- Integration suite covers full successful staged run and representative partial-stage reruns in `memory` and `disk` modes.
- Integration suite covers expected failures such as invalid constraints, missing persisted artifacts in `disk` mode, and mode-incompatible option combinations.
- Failure playbooks document triage steps and remediation for common failure classes, including mode-specific resume constraints.
- Test and playbook outputs are machine-readable and human-readable.

### Unit tests
- Integration tests for stage failure with correct abort and report behavior and failure-report schema and required triage metadata.
- Integration tests for full staged calibration completion with default and override mode paths.

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Keep full-run integration fixtures bounded for repeatable runtime.
- Separate deterministic orchestration tests from expensive statistical validations.
- Follow the combined run-contract guidance in JAN-122 and JAN-130.
