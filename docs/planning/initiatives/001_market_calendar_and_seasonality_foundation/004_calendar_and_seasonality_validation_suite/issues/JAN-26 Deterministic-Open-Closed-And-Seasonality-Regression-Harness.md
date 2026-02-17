## Deterministic Open Closed And Seasonality Regression Harness

### Description
Build a deterministic regression harness that executes fixture-driven checks for market state classification and seasonality factor outputs.

### Acceptance Criteria
- Harness evaluates expected `open` and `closed` outcomes for fixture timestamps
- Harness evaluates expected seasonal factor outputs with configurable numeric tolerance
- Harness output clearly reports pass and fail cases with fixture identifiers
- Harness runs can be invoked from both Rust and Python test environments

### Unit tests
- Failure-path tests confirming clear diagnostics on mismatches
- Golden tests that verify harness behavior on known fixture sets and determinism tests ensuring identical reports across repeated runs

### Notes/Constraints
- Keep harness output machine-readable for CI integration
- Do not duplicate assertion logic separately in each language stack
