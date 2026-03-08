## Pricing Execution Inputs And Native Runner Contract

### Description
Introduce the missing executable pricing workflow contract between the public Python request model and the Rust pricing engine so orchestrator entrypoints can run real pricing work instead of stopping at validation.

### Acceptance Criteria
- A canonical pricing execution input exists for the orchestrated pricing path and is separate from the baseline request and response contract when extra runtime inputs are required.
- The pricing execution path preserves known-scheduled and surprise news intensity inputs separately and does not collapse them into one lossy combined `nu_h` value where the model semantics require distinct components.
- One canonical Rust pricing runner exists for the public vanilla workflow and returns the existing pricing response plus optional run diagnostics and calibration hooks when requested.
- PyO3 exposes a thin pricing execution binding that maps the canonical pricing runner into the Python package without duplicating pricing logic in Python.
- Documentation explains how hourly interval factors feed the pricing path, including when the hourly-only shared provider is sufficient and when explicit execution inputs are required.
- Entrypoint-facing docs use corrected known-surprise news semantics and open-jump versus closed-gap terminology where exposed.

### Unit tests
- Contract tests for pricing execution input validation, including rejection of missing or inconsistent known versus surprise news intensity data.
- Integration tests for the Rust pricing runner and the PyO3 bridge on representative pricing fixtures.
- Tests covering optional run diagnostics and calibration hook selection through the execution path.
- Regression tests confirming that the orchestrated pricing path uses one canonical runner instead of duplicate pricing assembly logic.

### Notes/Constraints
- Keep the public baseline pricing request and response contract focused on user-facing inputs and outputs.
- Do not hide known versus surprise news inputs behind a combined hourly factor field when the execution path needs the split semantics.
- Keep the Python layer orchestration-focused and move engine-specific work into shared Rust services.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
