## ISSUE-003: Packaging Smoke And Installability Gates

### Description
Add packaging smoke-test gates that validate installation and basic runtime command execution for the packaged Python interface.

### Acceptance Criteria
- CI includes package build and install smoke checks for v0.1 workflows.
- Smoke checks validate importability and minimal CLI execution.
- Failures provide concise diagnostics tied to packaging stage.
- Packaging gate behavior is documented in release-readiness docs.

### Unit tests
- Smoke tests for package build and install commands.
- Tests for post-install import and CLI availability checks.
- Tests for expected failure behavior on broken package artifacts.

### Notes/Constraints
- Keep smoke checks lightweight for reliable CI runtimes.
- Avoid deep integration coverage in packaging smoke stage.
