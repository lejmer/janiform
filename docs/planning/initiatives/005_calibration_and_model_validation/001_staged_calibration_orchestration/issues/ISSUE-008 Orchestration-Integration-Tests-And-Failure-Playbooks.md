## ISSUE-008: Orchestration Integration Tests And Failure Playbooks

### Description
Add integration tests and failure playbooks for staged calibration to ensure clear handling of success paths, expected failures, and operator triage workflows.

### Acceptance Criteria
- Integration suite covers full successful staged run and representative partial-stage reruns.
- Integration suite covers expected failures such as invalid constraints and missing artifacts.
- Failure playbooks document triage steps and remediation for common failure classes.
- Test and playbook outputs are machine-readable and human-readable.

### Unit tests
- Integration tests for full staged calibration completion.
- Integration tests for stage failure with correct abort and report behavior.
- Tests for failure-report schema and required triage metadata.

### Notes/Constraints
- Keep full-run integration fixtures bounded for repeatable runtime.
- Separate deterministic orchestration tests from expensive statistical validations.
