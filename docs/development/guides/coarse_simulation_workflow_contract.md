# Coarse Simulation Workflow Contract

This guide defines the public Rust workflow contract for time-indexed coarse-interval
simulation paths.

## Scope

The contract covers:

- a time-anchored coarse simulation run request
- explicit interval semantics for `Delta` and horizon indexing
- factor-alignment validation rules for compatible interval grids
- machine-readable coarse path artifacts with interval boundary timestamps
- deterministic metadata and provenance for validation and analytics workflows

This workflow contract sits above the sufficient-statistics risk sampler contract.
The lower-level risk sampler types remain available for direct validation and engine
tests, while the coarse simulation workflow contract is the simulation-facing boundary.

## Rust Contract

Canonical Rust types live in `janiform_core::simulation`:

- `CoarseSimulationModelInputs`
- `CoarseSimulationRunRequest`
- `CoarseSimulationPathRecord`
- `CoarseSimulationPathOutputSchemaMetadata`
- `CoarseSimulationPathProvenance`
- `CoarseSimulationPathDiagnostics`
- `CoarseSimulationPath`
- `run_coarse_simulation`

Validation entrypoints:

- `CoarseSimulationModelInputs::try_new`
- `CoarseSimulationModelInputs::validate`
- `CoarseSimulationRunRequest::try_new`
- `CoarseSimulationRunRequest::validate`
- `CoarseSimulationPathRecord::try_new`
- `CoarseSimulationPath::try_new`
- `CoarseSimulationPath::validate`

## Model Inputs

`CoarseSimulationModelInputs` wraps the shared sufficient-statistics risk inputs and adds
the simulation-specific data needed to generate direct jump components inside the workflow:

- `risk_model_inputs`
- `news_process_configuration`
- `calendar_registry_artifact`
- `exchange_code`

The news-process payload carries the shared mark-law and component scaling configuration used for
both Hawkes activity loading and direct return-jump generation. The calendar payload is resolved
through the canonical exchange-calendar service so open-window `M` and closed-window `G` routing
follow the same market-state semantics used elsewhere in the model.

## Time Anchor And Interval Semantics

The request contract requires:

- `start_time_utc_nanos`
- `interval_delta`
- `horizon_interval_count`

These fields define a fixed-width half-open UTC interval grid:

- interval `0` is `[start_time_utc_nanos, start_time_utc_nanos + Delta)`
- interval `k` starts at `start_time_utc_nanos + k * Delta`
- the path horizon ends at `start_time_utc_nanos + horizon_interval_count * Delta`

Policy identifiers:

- `COARSE_SIMULATION_INTERVAL_TIMING_POLICY`
  - value:
    `start_time_utc_anchor_plus_fixed_interval_delta_half_open_utc_boundaries.v1`
- `COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY`
  - value:
    `interval_index_and_explicit_utc_boundary_timestamps.v1`
- `COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY`
  - value:
    `interval_delta_must_map_to_an_exact_fixed_width_utc_nanosecond_grid.v1`

`interval_delta` is interpreted in hours and must map to an exact UTC nanosecond
duration. This keeps boundary timestamps explicit and avoids hidden timestamp
reconstruction rules beyond the required start anchor.

## Factor Alignment Rules

The coarse workflow reuses the lower-level deterministic factor payload:

- `SufficientStatisticsRiskIntervalFactors`

Alignment is validated by:

- requiring `interval_delta == interval_factors.interval_delta`
- requiring `horizon_interval_count <= interval_factors.s_h.len()`
- interpreting `interval_factors` row `0` as aligned to the requested
  `start_time_utc_nanos`
- requiring `interval_factors.nu_h_known * Delta` to match the count of known scheduled
  news timestamps that fall inside each requested interval
- rejecting implicit factor interpolation or resampling

Policy identifiers:

- `COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY`
  - value:
    `factor_row_zero_anchors_to_start_time_utc_and_rows_progress_in_request_interval_order_without_implicit_resampling.v1`
- `SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY`
  - value:
    `explicit_interval_aligned_s_h_and_news_intensity_inputs_without_implicit_resampling.v1`

## Path Artifact

`CoarseSimulationPath` includes:

- top-level start and end timestamps
- interval-level records with:
  - `interval_index`
  - `interval_start_utc_nanos`
  - `interval_end_utc_nanos`
  - sampled sufficient statistics
  - sampled return components
- top-level initial and terminal Hawkes excitation state
- deterministic metadata and provenance blocks
- deterministic diagnostics for downstream validation and analytics

Interval return components are generated inside `run_coarse_simulation`:

- sufficient statistics are sampled from the exponential-affine transform slices
- known-news timestamps are taken from `news_process_configuration`
- surprise-news arrivals are sampled from `nu_h_surprise`
- shared severity and coupled jump marks are sampled from the shared news-process mark law
- market state is resolved per news timestamp through the calendar service
- routed open-window jumps accumulate into `open_jump_return`
- routed closed-window jumps accumulate into `closed_gap_return`

Schema identifiers:

- `COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION`
  - value: `simulation.coarse_interval.run_request.v1`
- `COARSE_SIMULATION_PATH_SCHEMA_VERSION`
  - value: `simulation.coarse_interval.path.v1`
- `COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION`
  - value: `simulation.coarse_interval.path_record.v1`
- `COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION`
  - value: `simulation.coarse_interval.path_output_schema_metadata.v1`
- `COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION`
  - value: `simulation.coarse_interval.path_provenance.v1`
- `COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION`
  - value: `simulation.coarse_interval.path_diagnostics.v1`

Artifact policies:

- `COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY`
  - value:
    `time_indexed_coarse_interval_simulation_path_for_validation_and_analytics.v1`
- `COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY`
  - value: `exclude_environment_specific_metadata.v1`

## Relationship To The Sufficient-Statistics Risk Sampler

The sufficient-statistics risk sampler contract remains available in
`janiform_core::risk` for:

- internal transform-slice execution
- validation suites
- scale-consistency harnesses
- other lower-level engine tests

The coarse simulation workflow contract is the preferred simulation-facing
artifact boundary because it carries explicit time anchoring and interval
boundary timestamps that the sampler-only contract does not.
