## Command Reference Documentation And Practical Examples

### Description
Write command reference documentation and practical examples for common CLI workflows to support onboarding and daily usage, with calibration examples that match normal market-professional expectations.

### Acceptance Criteria
- Documentation includes command synopsis, options, and example invocations.
- Examples cover simulation, calibration, and pricing tasks.
- Calibration documentation includes `--execution-mode {disk,memory}` semantics, CLI default `disk`, and explicit override examples.
- Documentation includes machine-readable output mode examples.
- Docs remain aligned with implemented command contracts.
- Calibration examples show users how to supply tick data files, session definitions, known-news calendars, and optional option-surface files.
- Advanced staged execution JSON examples, if retained, are explicitly labeled as advanced or internal workflows.

### Unit tests
- Documentation checks for required sections and command references, Smoke checks that documented example commands remain valid, and consistency between docs and CLI help output.
- Validation that calibration command docs include execution-mode defaults and override examples.
- Validation that primary calibration docs include at least one market-data-first example using external files rather than only internal execution-input documents.

### Notes/Constraints
- Keep examples realistic and copy-pastable.
- Avoid undocumented flags or behavior in published examples.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
