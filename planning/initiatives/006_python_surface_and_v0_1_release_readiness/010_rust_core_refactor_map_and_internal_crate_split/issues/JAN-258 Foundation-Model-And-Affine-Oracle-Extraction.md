## Foundation Model And Affine Oracle Extraction

### Description
Extract the low-level foundational and model-kernel code into dedicated internal crates so the rest of the workspace depends on explicit shared kernels instead of reaching directly into a monolithic `janiform`.

### Implementation details
- Introduce internal crates for:
  - foundation utilities such as `complex_math` and `randomness`
  - model-kernel domains such as `hawkes`, `news`, `nig`, `return_decomposition`, and `risk_neutral`
  - the affine-oracle solver and contracts
- Move these areas with minimal behavior change and preserve the current Rust and Python behavior through `janiform` re-exports.
- Remove any accidental dependencies from these low-level crates back into workflow, validation, or governance code.
- Normalize shared utilities so downstream crates consume the extracted crates rather than recreating equivalent helpers.

### Acceptance Criteria
- Foundational utilities, model kernels, and affine-oracle code live in explicit internal crates with no dependencies on calibration runtime, validation, or governance crates.
- `janiform` continues to re-export the intended facade during migration.
- In-repo callers are updated to consume the new internal crate boundaries rather than the pre-split module paths.
- Compile and test behavior remains unchanged other than path and ownership cleanup.

### Unit tests
- Existing kernel and solver unit tests moved or mirrored into the new crates.
- Re-export contract tests confirming `janiform` still exposes the expected public surface.
- Dependency smoke tests asserting the extracted low-level crates do not depend on higher-level workflow crates.

### Notes/Constraints
- This extraction should happen before engine, calibration, or governance splits.
- Prefer behavior-preserving moves over opportunistic redesign.
- For context, see the full plan: docs/development/guides/`rust_internal_crate_graph_and_umbrella_facade_policy.md`.
