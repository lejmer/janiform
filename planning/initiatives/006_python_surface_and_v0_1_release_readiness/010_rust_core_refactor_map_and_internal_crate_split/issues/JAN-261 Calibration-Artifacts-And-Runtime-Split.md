## Calibration Artifacts And Runtime Split

### Description
Split calibration into separate artifact and runtime crates so persistent workflow state, artifact schemas, checkpointing, and reload logic are no longer mixed with stage execution, optimizer integration, and market-data orchestration.

### Implementation details
- Introduce one internal crate for calibration artifacts and persistence concerns:
  - artifact contracts
  - artifact manifests
  - dataset snapshots
  - checkpointing
  - reload and lookup helpers
- Introduce one internal crate for calibration runtime concerns:
  - stage execution
  - staged runner
  - execution bridge
  - market-data preprocessing
  - optimizer adapters
  - runtime objectives and adapters
- Physically move artifact-owned and runtime-owned source files, crate-local helpers, and unit tests into their destination crates. Do not leave them implemented under `crates/core` behind `#[path = "..."]` or equivalent indirection.
- Ensure runtime depends on artifacts, but artifacts do not depend on runtime orchestration.
- Update `janiform-core` and pybind-facing callers to consume the new boundaries through curated re-exports.

### Acceptance Criteria
- Calibration artifact logic and calibration runtime logic are separate internal crates with one-way dependencies.
- Artifact and runtime code are no longer physically co-owned under `crates/core`.
- The calibration runtime no longer owns artifact persistence internals directly.
- Artifact crates remain free of optimizer and stage-execution concerns.
- Existing staged calibration behavior remains intact through the migration.

### Unit tests
- Artifact manifest, reload, and checkpoint tests continue under the artifact crate.
- Staged runner, execution-bridge, and stage-execution tests continue under the runtime crate.
- Integration tests verify runtime still uses the extracted artifact crate correctly.

### Notes/Constraints
- Calibration is one of the largest current ownership hotspots. Do not move it as one giant extracted crate.
- Keep the split aligned with responsibility, not just with file count.
- For context, see the full plan: docs/development/guides/`rust_internal_crate_graph_and_umbrella_facade_policy.md`.
