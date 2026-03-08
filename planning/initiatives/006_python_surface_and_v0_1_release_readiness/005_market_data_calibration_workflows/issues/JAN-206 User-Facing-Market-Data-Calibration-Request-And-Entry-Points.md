## User Facing Market Data Calibration Request And Entry Points

### Description
Define the user-facing Python and CLI calibration surfaces for real market-data workflows so users provide files, session context, and known-news inputs rather than internal staged execution payloads.

### Implementation details
- Define a new top-level calibration request model for market-data-first workflows with fields for tick-data source, optional option-surface source, session configuration, known-news inputs, column mapping, workflow scope, and output preferences.
- Define CLI entry points and flag naming for the new workflow, including how it coexists with the existing advanced staged-calibration command path.
- Make the user-facing request the default recommended calibration path in planning docs, while preserving staged execution input as an advanced or internal contract.
- Define provenance requirements so every run can report the original user-supplied files and normalization policy selections.
- Specify how execution-mode defaults interact with the new workflow, including when disk-backed persistence is implicit or required.
- Document the separation between user-facing request validation and lower-level staged execution validation.

### Acceptance Criteria
- Planning defines one clear user-facing calibration request for file-driven workflows.
- CLI planning exposes a calibration path that matches market-user expectations and does not require hand-authored staged execution JSON.
- Advanced staged-calibration entry points remain available but are explicitly positioned as expert or internal surfaces.
- Provenance fields are defined for all primary user inputs and derived workflow settings.

### Unit tests
- Contract validation tests for required user-facing request fields and invalid combinations.
- CLI argument-mapping tests for the new entry points and advanced-mode separation.
- Validation coverage for provenance metadata completeness and deterministic serialization of the new request.

### Notes/Constraints
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
- Do not add compatibility wrappers that keep the current internal contract as the default public UX.
- Keep the staged execution engine canonical and treat the new request as a higher-level boundary above it.
