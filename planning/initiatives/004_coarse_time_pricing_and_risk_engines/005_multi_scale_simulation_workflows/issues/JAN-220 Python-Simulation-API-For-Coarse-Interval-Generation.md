## Python Simulation API For Coarse Interval Generation

### Description
Expose the new coarse-interval simulation workflow through the public Python simulation module so users can generate interval paths as intentionally as they can already generate seeded tick tapes.

The missing functionality is not only a Python-surface problem, but once the Rust core workflow exists it should be surfaced through `janiform.simulation` rather than left hidden behind private bindings or risk-only helper contracts.

### Acceptance Criteria
- The public Python simulation surface includes typed request and response models for coarse-interval simulation.
- Python users can execute coarse simulation from `janiform.simulation` without reaching into private modules or risk-internal helpers.
- The seeded-tick simulation API remains available and clearly distinct from the coarse-interval workflow.
- Public docs and API-surface tests include the new coarse simulation entrypoint and contracts.
- Python bindings and serialization remain thin wrappers over the Rust core workflow rather than reimplementing simulation logic in Python.

### Unit tests
- Public API surface tests covering the new coarse simulation symbols.
- Python contract tests for request validation, response validation, and round-trip serialization.
- Rust-Python parity tests for representative coarse simulation scenarios.

### Notes/Constraints
- Keep the workflow in `janiform.simulation` rather than introducing a user-facing `janiform.risk` execution API for this path.
- Avoid compatibility shims if the existing risk-only contract needs to be replaced by a simulation-oriented boundary.
