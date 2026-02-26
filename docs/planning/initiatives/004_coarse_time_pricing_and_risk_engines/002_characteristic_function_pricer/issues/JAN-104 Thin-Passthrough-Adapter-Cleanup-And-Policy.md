## Thin Passthrough Adapter Cleanup And Policy

### Description
Inventory and clean up consumer-specific adapter functions that add no behavior beyond forwarding to a shared function. The current pattern increases surface area and maintenance cost without adding validation, policy defaults, type conversion, telemetry, or boundary hardening.

Identified zero-behavior passthrough targets:
- `crates/core/src/pricing.rs`
  - `load_hourly_factor_contract_for_pricing`
  - `compose_nig_trading_time_exponent_for_pricing_with_edge_mode`
  - `compose_nig_trading_time_exponent_for_coarse_transform_integration_with_edge_mode`
  - `compose_total_return_characteristic_function_for_pricing_with_edge_mode`
  - `compose_total_return_characteristic_function_for_coarse_transform_integration_with_edge_mode`
  - `compose_total_return_characteristic_function_from_oracle_output_for_pricing`
  - `compose_total_return_characteristic_function_from_oracle_output_for_coarse_transform_integration`
  - `evolve_hawkes_state_for_coarse_interval_step`
- `crates/core/src/simulation.rs`
  - `load_hourly_factor_contract_for_simulation`
  - `sample_nig_trading_time_increment_for_simulation_with_edge_mode`
  - `evolve_hawkes_state_for_simulation_step`

Potential follow-up candidates with minor argument-shape adaptation (`Some(...)` or field extraction) should be reviewed separately after this issue.

### Acceptance Criteria
- Every function in the target inventory is either:
  - removed and call sites use the shared function directly, or
  - replaced by a non-duplicative export strategy (`pub use`) when naming must stay stable.
- No retained adapter in this scope remains a pure passthrough.
- Public API impact is explicitly documented, including migration notes if removals are breaking.
- Pricing, risk, and simulation docs no longer imply consumer wrappers are required when behavior is shared.

### Unit tests
- Existing behavioral tests pass with no regression in pricing, simulation, and oracle integration suites.
- Tests that only verify wrapper equivalence are removed or rewritten to validate real boundary behavior.
- Add one regression guard that fails if newly introduced `*_for_*` public functions are pure passthroughs without documented boundary behavior.

### Notes/Constraints
- Do not remove adapters that currently inject required defaults or policy choices in this issue unless the shared API is refactored first.
- Keep shared implementation paths canonical and single-sourced.
- Prefer explicit docs for boundary intent over speculative wrapper creation.
