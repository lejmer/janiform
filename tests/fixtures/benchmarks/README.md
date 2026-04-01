# Benchmark Scenario Catalog Fixtures

This directory stores versioned benchmark scenario workload profiles shared by coarse-time
benchmark harnesses.

The microbenchmark layer reuses these checked-in inputs. For benchmark taxonomy guidance and local
Criterion plus `pytest-benchmark` commands, see
`docs/development/guides/benchmark_taxonomy_and_microbenchmark_workflows.md`.

## File

- `benchmark_scenario_catalog.json`
- `anchored_pricing_reference_policy.json`
- `anchored_pricing_reference_baseline_v1.json`
- `oracle_pricer_risk_benchmark_policy.json`
- `coarse_fine_consistency_tolerance_policy.json`
- `oracle_pricer_risk_performance_baseline_v1.json`
- `oracle_pricer_risk_performance_baseline_v1_1.json`
- `real_data_seeded_runtime_benchmark_fixture.json`
- `real_data_seeded_runtime_benchmark_policy.json`
- `real_data_seeded_runtime_baseline_v1.json`
- `real_data_seeded_runtime_smoke_fixture.json`
- `real_data_seeded_runtime_smoke_policy.json`
- `real_data_seeded_runtime_smoke_baseline_v1.json`
- `real_data_seeded_runtime_smoke_report.json`

## Contract

- `schema_version`: `benchmark.scenario.catalog.v1`
- `catalog_id`: `coarse_time_engine_benchmark_scenarios_v1`
- Workload scales: `small`, `medium`, `large`
- Interval grid coverage includes an hourly default profile and at least one sub-hour profile for sensitivity checks.
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
- Anchored pricing reference policy fields include:
  - `schema_version`: `benchmark.anchored_pricing_reference.policy.v1`
  - `simulation_reference`: explicit seeded-reference engine, payoff estimator, and path-count policy
  - `diagnostic_requirements`: required observed-boundary, anchor-alignment, local-martingale,
    refinement, and numeric-quality coverage flags
  - `scenario_ids`: governed anchored benchmark scenarios that must stay aligned with the checked-in
    baseline artifact
- Anchored pricing reference baseline fields include:
  - `schema_version`: `benchmark.anchored_pricing_reference.baseline.v1`
  - `source_policy_id` and `source_policy_version`: checked-in linkage to the governing policy
  - `reference_construction_policy`: explicit statement that the seeded reference uses a zero-factor
    one-interval synthetic case with zero Monte Carlo variance
  - `regression_tolerances`: deterministic native-price and native-minus-seeded-gap regression
    tolerances for the explicitly invoked benchmark path
  - `cases`: per-scenario anchored boundary inputs, anchor alignment, admissibility expectations,
    local-martingale diagnostic mode, deviation-grid diagnostics, and numeric-quality summaries
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
    `throughput_operations_per_second`, `measured_operation_count`, and
    `interval_delta` metadata for profile-matrix comparability
- Real-data seeded runtime benchmark fixture fields include:
  - `schema_version`: `benchmark.real_data_seeded_runtime.fixture.v1`
  - checked-in unsigned EUR/USD parquet slice controls:
    `source_tick_data_path`, `source_tick_data_format`,
    `source_tick_data_row_offset`, `source_tick_data_row_count`,
    `source_timestamp_column`, `source_price_column`, and
    `source_trade_sign_column`
  - execution controls for the repaired high-level runtime path:
    `market_profile`, `coarse_interval`, `coarse_horizon_interval_count`,
    `seeded_horizon_interval_count`, and `simulation_seed`
  - representative Python access patterns used to attribute boundary and
    materialization cost through `materialization_access_patterns`
- Real-data seeded runtime policy fields include:
  - `schema_version`: `benchmark.real_data_seeded_runtime.threshold_policy.v1`
  - explicit event-regime realism thresholds for coarse and seeded event-rate ratios
  - runtime, Python materialization, boundary transport, and retained-memory
    regression thresholds per fixture
- Real-data seeded runtime baseline fields include:
  - `schema_version`: `benchmark.real_data_seeded_runtime.baseline.v1`
  - accepted event-count, runtime, materialization, transport, and retained-memory
    values for each benchmark fixture
  - `memory_metric_semantics` documents that `native_estimated_retained_bytes`
    is an estimated retained-memory proxy and not true peak RSS
- Real-data seeded runtime smoke artifacts mirror the same contracts with a much smaller
  checked-in workload so the default Python test suite can validate report and artifact wiring
  without rerunning the full representative benchmark workload.
- Regenerate `real_data_seeded_runtime_smoke_report.json` with
  `python3 scripts/generate_real_data_seeded_runtime_smoke_report.py` when the smoke runtime
  report contract or fixture changes.
- Regression drift attribution report fields include:
  - `schema_version`: `benchmark.regression_drift_attribution.report.v1`
  - `classification_policy`: explicit drift-category rules for numerical drift, performance drift,
    and scenario changes
  - `comparison_context`: optional links to prior coarse-fine and performance baseline run versions
  - `entries`: per-finding triage records with `scenario_id`, `component_id`, `threshold_id`,
    metric identifiers, and deltas
  - `output_schema_metadata`: required-field lists for deterministic machine-readable validation

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

`real_data_seeded_runtime_benchmark_policy.json` governs the real-data seeded runtime path
separately from the engine-only benchmark suites so model-regime failures can be distinguished from
boundary transport, Python materialization, and retained-memory regressions.

`anchored_pricing_reference_policy.json` and
`anchored_pricing_reference_baseline_v1.json` govern the native-pricer versus seeded-reference
comparison path for anchored pricing separately from the default Rust and Python suites. The
expensive execution path is intentionally opt-in and runs only through the ignored Rust validation
benchmark.

## Performance baseline ownership

- Ownership policy: `initiative_004_project_004_maintainers.v1`
- Owner team: `coarse_time_pricing_and_risk_engines`
- On-call alias: `janiform-owners`

## Performance baseline refresh process

1. Run the oracle pricer risk benchmark harness with deterministic execution config and fixed
   benchmark catalog and policy fixtures.
2. Capture the baseline artifact from the benchmark report with:
   `capture_oracle_pricer_risk_performance_baseline_artifact(...)`.
3. Bump `baseline_version` and update `ownership_metadata.refresh_process` only when process links
   change.
4. Save the updated JSON artifact in this directory and keep prior versions for drift comparisons.
5. Validate with:
   `cargo test -p janiform-governance --test performance_baseline_capture_and_storage`.
