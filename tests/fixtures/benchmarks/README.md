# Benchmark Scenario Catalog Fixtures

This directory stores versioned benchmark scenario workload profiles shared by coarse-time
benchmark harnesses.

## File

- `benchmark_scenario_catalog.json`
- `oracle_pricer_risk_benchmark_policy.json`

## Contract

- `schema_version`: `benchmark.scenario.catalog.v1`
- `catalog_id`: `coarse_time_engine_benchmark_scenarios_v1`
- Workload scales: `small`, `medium`, `large`
- Profile fields include:
  - `model_state`
  - `interval_settings`
  - `portfolio_shape`
  - `run_length`
  - `component_workload`
- Oracle pricer risk benchmark policy fields include:
  - `rationale`: required documentation for why each parameter group is set as recorded
  - `model_parameters`: Hawkes and Gamma inputs used by benchmark invocations
  - `oracle_solver_probes`: transform probe and factor ranges used in oracle and risk paths
  - `pricing_input_ranges`: pricing workload ranges for market inputs and Fourier settings
  - `seed_partition`: deterministic component seed salts

## Determinism

The loader applies deterministic profile ordering by:

1. workload scale (`small` then `medium` then `large`)
2. `profile_id` ascending

## Scope

These fixtures are bounded for repeatable CI execution while remaining representative enough to
exercise oracle, pricing, and sufficient-statistics risk sampling workloads.

## Policy rationale

`oracle_pricer_risk_benchmark_policy.json` is the canonical source for baseline benchmark parameter
values. The `rationale` object is required and documents why each exact parameter group exists, so
the harness does not rely on undocumented hardcoded constants.
