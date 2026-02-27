# Benchmark Scenario Catalog Fixtures

This directory stores versioned benchmark scenario workload profiles shared by coarse-time
benchmark harnesses.

## File

- `benchmark_scenario_catalog.json`
- `oracle_pricer_risk_benchmark_policy.json`
- `coarse_fine_consistency_tolerance_policy.json`
- `oracle_pricer_risk_performance_baseline_v1.json`
- `oracle_pricer_risk_performance_baseline_v1_1.json`

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
- Coarse fine consistency tolerance policy fields include:
  - `policy_version` and `change_log`: required version history with rationale metadata
  - `interval_tolerance_bands`: interval-dependent tolerances keyed by `delta_upper_bound`
  - tolerance coverage for:
    - oracle output differences
    - pricing output deltas
    - risk path summary metric deltas
  - `governance_scope_policy`: explicit engine-level-only scope for benchmark regression workflows
- Oracle pricer risk performance baseline fields include:
  - `baseline_id` and `baseline_version`: versioned storage identity for comparison runs
  - `schema_metadata`: schema family and forward-extension policy for machine-readable loading
  - `ownership_metadata`: team ownership and refresh process metadata
  - `capture_policy`: baseline extraction policy for concise component metrics
  - `output_schema_metadata`: machine-readable required-field lists for baseline and record payloads
  - `source_report`: benchmark report provenance (`catalog_id`, `policy_id`, and policy metadata)
  - `profile_records`: per-profile component metrics (`mean_latency_seconds`,
    `throughput_operations_per_second`, `measured_operation_count`)

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

`coarse_fine_consistency_tolerance_policy.json` centralizes versioned regression thresholds for
engine-level coarse fine workflows and supports interval-grid bands so policy updates do not require
harness code changes.

## Performance baseline ownership

- Ownership policy: `initiative_004_project_004_maintainers.v1`
- Owner team: `coarse_time_pricing_and_risk_engines`
- On-call alias: `janiform-core-owners`

## Performance baseline refresh process

1. Run the oracle pricer risk benchmark harness with deterministic execution config and fixed
   benchmark catalog and policy fixtures.
2. Capture the baseline artifact from the benchmark report with:
   `capture_oracle_pricer_risk_performance_baseline_artifact(...)`.
3. Bump `baseline_version` and update `ownership_metadata.refresh_process` only when process links
   change.
4. Save the updated JSON artifact in this directory and keep prior versions for drift comparisons.
5. Validate with:
   `cargo test -p janiform-core --test performance_baseline_capture_and_storage`.
