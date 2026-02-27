# Benchmark Scenario Catalog Fixtures

This directory stores versioned benchmark scenario workload profiles shared by coarse-time
benchmark harnesses.

## File

- `benchmark_scenario_catalog.json`

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

## Determinism

The loader applies deterministic profile ordering by:

1. workload scale (`small` then `medium` then `large`)
2. `profile_id` ascending

## Scope

These fixtures are bounded for repeatable CI execution while remaining representative enough to
exercise oracle, pricing, and sufficient-statistics risk sampling workloads.
