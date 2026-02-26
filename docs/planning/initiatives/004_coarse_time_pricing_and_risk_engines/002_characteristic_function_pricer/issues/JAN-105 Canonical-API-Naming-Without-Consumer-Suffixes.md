## Canonical API Naming Without Consumer Suffixes

### Description
Follow up JAN-104 by removing consumer-oriented `*_for_*` naming from the core pricing and simulation APIs now that no external compatibility constraints exist.

The goal is to keep one canonical public API per behavior, expressed with domain-oriented names rather than use-case suffixes.

Target scope:
- `crates/core/src/pricing.rs`
- `crates/core/src/simulation.rs`
- related Rust integration tests and docs in this initiative

Legacy names removed in scope:
- `compose_nig_trading_time_exponent_for_pricing`
- `compose_nig_trading_time_exponent_for_pricing_with_edge_mode`
- `compose_nig_trading_time_exponent_for_coarse_transform_integration`
- `compose_nig_trading_time_exponent_for_coarse_transform_integration_with_edge_mode`
- `compose_total_return_characteristic_function_for_pricing`
- `compose_total_return_characteristic_function_for_pricing_with_edge_mode`
- `compose_total_return_characteristic_function_for_coarse_transform_integration`
- `compose_total_return_characteristic_function_for_coarse_transform_integration_with_edge_mode`
- `compose_total_return_characteristic_function_from_oracle_output_for_pricing`
- `compose_total_return_characteristic_function_from_oracle_output_for_coarse_transform_integration`
- `sample_nig_trading_time_increment_for_simulation`
- `sample_nig_trading_time_increment_for_simulation_with_edge_mode`
- `load_hourly_factor_contract_for_pricing`
- `load_hourly_factor_contract_for_simulation`
- `evolve_hawkes_state_for_coarse_interval_step`
- `evolve_hawkes_state_for_simulation_step`
- `project_hawkes_intensities_for_coarse_interval`
- `project_hawkes_intensities_for_simulation_interval`
- `solve_exponential_affine_transform_for_pricing`
- `solve_exponential_affine_transform_for_pricing_with_settings`

Canonical names in scope:
- `pricing::compose_nig_trading_time_exponent`
- `pricing::compose_nig_trading_time_exponent_with_edge_mode`
- `pricing::compose_total_return_characteristic_function`
- `pricing::compose_total_return_characteristic_function_with_edge_mode`
- `pricing::compose_total_return_characteristic_function_from_oracle_output`
- `simulation::sample_nig_trading_time_increment`
- `simulation::sample_nig_trading_time_increment_with_edge_mode`
- `pricing::load_hourly_factor_contract`
- `simulation::load_hourly_factor_contract`
- `pricing::evolve_hawkes_state`
- `simulation::evolve_hawkes_state`
- `pricing::project_hawkes_intensities_coarse_interval`
- `simulation::project_hawkes_intensities_simulation_interval`
- `pricing::solve_exponential_affine_transform`
- `pricing::solve_exponential_affine_transform_with_settings`

### Acceptance Criteria
- No public pricing or simulation API in `core` keeps `*_for_*` suffix naming after the refactor.
- Compatibility aliases introduced in JAN-104 are removed rather than retained.
- Canonical names are centralized around shared domain entrypoints.
- All in-repo call sites are migrated to canonical names in one change.
- Documentation in this initiative reflects canonical naming and no longer mentions compatibility aliases.
- Public API impact is explicitly documented as intentional breaking cleanup for pre-release code.

### Unit tests
- Existing pricing, simulation, and oracle integration suites pass after renaming.
- Any tests that only assert alias/wrapper equivalence are removed.
- Keep a regression guard that fails when new public `*_for_*` functions are introduced in pricing and simulation modules.

### Notes/Constraints
- Do not add new compatibility aliases in this issue.
- Prefer module boundaries and domain semantics over consumer-specific naming.
- Avoid introducing new OOP shells unless they carry real state or policy behavior.
- Keep behavior unchanged while collapsing naming surface area.
