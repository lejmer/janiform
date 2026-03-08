## Market Professional Examples Diagnostics And Workflow Coverage

### Description
Plan the user-facing examples, diagnostics, and regression coverage needed to make the new market-data calibration workflow understandable, trustworthy, and release-ready.

### Implementation details
- Define practical examples for calibrating from tick-data `csv`, tick-data `parquet`, and optional option-surface files with explicit session and known-news inputs.
- Define diagnostics summaries that explain what data was loaded, how it was normalized, what preprocessing steps ran, and which calibration scope was executed.
- Add workflow-level smoke and regression coverage for the new calibration path in both repository examples and release-readiness checks.
- Specify how advanced staged-calibration examples are documented alongside the new workflow without replacing it as the primary onboarding path.
- Align example fixtures, docs, CLI help, and release evidence so they describe the same user journey and terminology.
- Define failure-triage expectations for common onboarding mistakes such as missing columns, timezone ambiguity, or incompatible file content.

### Acceptance Criteria
- Planning includes realistic market-oriented examples for the primary calibration workflows.
- Diagnostics planning makes preprocessing and provenance visible enough for user trust and support triage.
- Workflow-level regression and smoke coverage exists for the new user-facing path.
- Advanced staged-calibration examples are documented as secondary expert workflows.

### Unit tests
- Example-validation and smoke-test planning for documented market-data calibration workflows.
- Diagnostics schema checks for required summary and provenance sections.
- Regression coverage for common onboarding failure cases and their user-facing messages.

### Notes/Constraints
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
- Keep examples realistic and recognizable to market users rather than only to Janiform contributors.
- Coordinate terminology with Project 002 command docs and Project 004 release-readiness docs.
