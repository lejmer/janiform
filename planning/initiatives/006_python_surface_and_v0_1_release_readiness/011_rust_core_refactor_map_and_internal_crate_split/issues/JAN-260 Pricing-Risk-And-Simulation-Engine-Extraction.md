## Pricing Risk And Simulation Engine Extraction

### Description
Split the runtime engine domains into explicit internal engine crates so pricing, risk sampling, and simulation become owned execution surfaces with clear dependencies on kernels and market-time infrastructure.

### Implementation details
- Introduce dedicated internal crates for:
  - pricing engine
  - risk engine
  - simulation engine
- Move engine contracts, execution logic, and engine-local helpers into those crates.
- Keep engines dependent only on:
  - foundation utilities
  - model-kernel crates
  - affine-oracle crate where needed
  - market-time infrastructure where needed
- Ensure engines do not depend on calibration runtime, validation, or governance crates.
- Preserve the current umbrella-core and pybind-facing APIs through curated re-exports during migration.

### Acceptance Criteria
- Pricing, risk, and simulation code are no longer implemented as modules inside one monolithic crate.
- Engine ownership is explicit and compile boundaries reflect real runtime domains.
- Engine crates do not take dependencies on validation, governance, or calibration orchestration crates.
- Existing behavior and contracts remain stable through the umbrella facade.

### Unit tests
- Engine unit and integration tests move with the extracted crates.
- Cross-engine integration tests continue to verify the shared-kernel behavior where pricing and simulation reuse common model primitives.
- Facade re-export tests confirm the public surface remains intact.

### Notes/Constraints
- Do not use this split to redesign engine semantics.
- Keep extraction behavior-preserving first, then consider local cleanup once boundaries hold.
- For context, see the full plan: docs/development/guides/`rust_internal_crate_graph_and_umbrella_facade_policy.md`.
