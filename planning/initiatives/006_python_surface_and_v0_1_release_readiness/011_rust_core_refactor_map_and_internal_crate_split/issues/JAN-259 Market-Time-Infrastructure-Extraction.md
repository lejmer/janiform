## Market-Time Infrastructure Extraction

### Description
Extract `calendar`, `seasonality`, and `hourly_factors` into a dedicated internal market-time crate so session handling and factor materialization become an explicit shared subsystem rather than ambient utility code inside `janiform-core`.

### Implementation details
- Introduce a `market-time` internal crate containing:
  - calendar definitions, registry logic, and validation
  - seasonality estimation, composition, and diagnostics
  - hourly-factor contracts and materialization helpers
- Physically move the owned Rust source files, crate-local helpers, and unit tests for these domains into the new crate. Do not treat `#[path = "..."]` compilation from `crates/core` or a Cargo-edge-only reshuffle as a completed extraction.
- Update pricing, simulation, calibration, and validation callers to depend on the extracted market-time crate through the umbrella facade or direct internal dependencies as appropriate.
- Keep session and factor contracts stable while moving ownership and compile boundaries.
- Audit for low-level utilities that should stay in foundation crates rather than remaining inside market-time.

### Acceptance Criteria
- Market-time infrastructure is owned by a dedicated internal crate.
- `calendar`, `seasonality`, and `hourly_factors` are no longer physically implemented under `crates/core`.
- Engine and workflow crates depend on market-time through an explicit boundary.
- No market-time code depends on calibration runtime, validation, or governance crates.
- The extracted crate remains usable for both pricing-time and simulation-time factor resolution.

### Unit tests
- Calendar, seasonality, and hourly-factor regression suites continue to pass against the extracted crate.
- Crate-local unit tests for the moved market-time domains live with the extracted crate rather than
  remaining under `crates/core`.
- Cross-crate integration tests confirm the extracted market-time crate still serves pricing, simulation, and calibration callers correctly.

### Notes/Constraints
- Keep this split centered on shared market-time infrastructure, not on workflow orchestration.
- Do not mix pybind or Python-specific concerns into the extracted crate.
- For context, see the full plan: docs/development/guides/`rust_internal_crate_graph_and_umbrella_facade_policy.md`.
