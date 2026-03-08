## Market Data Preprocessing And Staged Calibration Translation

### Description
Define the preprocessing and translation path that turns normalized market data, session context, and known-news inputs into the canonical staged calibration execution input used by Janiform internally.

### Implementation details
- Define translation from normalized tick and option-surface inputs into seasonality-stage, Hawkes-stage, and return-components-stage execution inputs.
- Specify preprocessing responsibilities such as session-boundary alignment, known-news overlay construction, candidate generation, and close-open gap derivation using the raw observations users provide.
- Define scope-aware behavior for common workflows like seasonality-only, full calibration without option surface, and full calibration with option surface.
- Define where deterministic defaults are allowed versus where explicit user overrides are required.
- Specify derivation diagnostics and provenance summaries so users can understand how Janiform transformed source files into staged calibration inputs.
- Ensure the translation layer feeds the existing staged calibration runner rather than creating a parallel execution path.

### Acceptance Criteria
- Planning defines one canonical translation pipeline from user-facing market-data inputs to staged calibration execution input.
- Stage-input derivation rules are explicit for session handling, known-news handling, and return-components preprocessing.
- Workflow-scope rules explain which source inputs are required for each supported calibration mode.
- Translation outputs include diagnostics and provenance sufficient for replay and triage.

### Unit tests
- Deterministic translation tests from normalized fixtures into staged calibration contracts.
- Scope-validation tests for seasonality-only, calibration without option surface, and calibration with option surface workflows.
- Regression tests for close-open gap derivation, session-boundary routing, and known-news overlay construction.
- Provenance and diagnostics tests for derived stage-input summaries.

### Notes/Constraints
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
- Reuse the existing staged calibration machinery instead of duplicating stage execution logic.
- Keep derivation steps inspectable so advanced users can debug what Janiform inferred from their files.
