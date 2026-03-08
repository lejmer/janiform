## ISSUE-003: Packaging Smoke And Installability Gates

### Description
Add packaging smoke-test gates that validate installation and basic runtime command execution for the packaged Python interface.

### Acceptance Criteria
- CI includes package build and install smoke checks for v0.1 workflows
- Smoke checks validate importability and minimal CLI execution
- Failures provide concise diagnostics tied to packaging stage
- Packaging gate behavior is documented in release-readiness docs
- Packaging smoke planning includes one minimal market-data calibration invocation using packaged commands and fixture files

### Unit tests
- Smoke tests for package build and install commands and post-install import and CLI availability checks
- Expected failure behavior on broken package artifacts
- Validation that packaged calibration smoke can locate fixture inputs and report missing-runtime-dependency failures clearly

### Notes/Constraints
- Keep smoke checks lightweight for reliable CI runtimes
- Avoid deep integration coverage in packaging smoke stage
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
