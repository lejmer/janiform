## ISSUE-008: Command Reference Documentation And Practical Examples

### Description
Write command reference documentation and practical examples for common CLI workflows to support onboarding and daily usage.

### Acceptance Criteria
- Documentation includes command synopsis, options, and example invocations.
- Examples cover simulation, calibration, and pricing tasks.
- Calibration documentation includes `--execution-mode {disk,memory}` semantics, CLI default `disk`, and explicit override examples.
- Documentation includes machine-readable output mode examples.
- Docs remain aligned with implemented command contracts.

### Unit tests
- Documentation checks for required sections and command references, Smoke checks that documented example commands remain valid, and consistency between docs and CLI help output.
- Validation that calibration command docs include execution-mode defaults and override examples.

### Notes/Constraints
- Keep examples realistic and copy-pastable.
- Avoid undocumented flags or behavior in published examples.
