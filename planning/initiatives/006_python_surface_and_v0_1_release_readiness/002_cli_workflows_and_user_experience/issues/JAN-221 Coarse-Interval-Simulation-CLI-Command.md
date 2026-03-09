## Coarse Interval Simulation CLI Command

### Description
Add a coarse-interval simulation command under the `simulation` CLI namespace once the Rust core and public Python coarse simulation workflow are available.

Today the CLI exposes only seeded tick simulation, which makes the command surface inconsistent with Janiform's multi-scale simulation design. The CLI should support direct interval-path generation for users who want simulation outputs at a chosen interval length `Delta` without replaying ticks.

### Acceptance Criteria
- The CLI exposes a coarse-interval simulation command under the `simulation` namespace alongside `seeded-ticks`.
- The command accepts a file-driven coarse simulation request contract and emits the canonical coarse simulation path artifact.
- The command follows existing CLI output-mode, help-text, and error-classification conventions.
- The command makes the distinction between fine-scale tick simulation and coarse-interval simulation explicit in help text and documentation.
- Command reference docs and practical examples show when to use coarse simulation versus seeded tick simulation.

### Unit tests
- Parser tests for the new simulation subcommand and required arguments.
- CLI tests for successful coarse simulation execution with fixture-backed inputs.
- Failure-path tests for malformed requests, incompatible interval-factor alignment, and execution failures surfaced from the workflow boundary.

### Notes/Constraints
- This command depends on the coarse simulation workflow defined in Initiative 004 and should not be implemented as a thin wrapper over risk-internal helper contracts.
- Keep the command in `simulation` rather than introducing a separate top-level namespace so the multi-scale simulation story stays discoverable.
