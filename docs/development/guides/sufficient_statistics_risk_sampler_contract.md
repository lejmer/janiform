# Sufficient Statistics Risk Sampler Contract

This guide defines the public v0.1 run request and path output contract for the
sufficient-statistics risk simulation pipeline.

## Scope

The contract covers:

- run request fields for seeded execution and interval settings
- required model inputs and deterministic interval factors
- interval-level output records with sufficient statistics and return components
- validation behavior for malformed or incomplete payloads

This contract intentionally excludes engine-internal temporary fields and
implementation-specific solver workspace values.

## Rust contract

Canonical Rust types live in `janiform_core::risk`:

- `SufficientStatisticsRiskIntervalFactors`
- `SufficientStatisticsRiskModelInputs`
- `SufficientStatisticsRiskRunRequest`
- `SufficientStatisticsIntervalStatistics`
- `SufficientStatisticsRiskReturnComponents`
- `SufficientStatisticsGeneratedIntervalReturn`
- `SufficientStatisticsRiskPathRecord`
- `SufficientStatisticsRiskPathOutputSchemaMetadata`
- `SufficientStatisticsRiskPathProvenance`
- `SufficientStatisticsRiskPathDiagnostics`
- `SufficientStatisticsRiskPath`

Validation entrypoints:

- `SufficientStatisticsRiskRunRequest::try_new`
- `SufficientStatisticsRiskPath::try_new`

Return generation entrypoint:

- `generate_interval_return_from_sampled_statistics`
  - consumes sampled `A_plus` and `A_minus` with market-state-routed `open_jump` and `closed_gap` inputs
  - reuses shared return-decomposition sampling for sell-side sign handling
  - supports optional component breakdown through
    `SufficientStatisticsRiskReturnGenerationOptions`

Return generation policies:

- `SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY`
- `SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_BREAKDOWN_POLICY`
- `SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY`

Streaming and batch execution entrypoints:

- `stream_interval_statistics_from_transform_slices`
- `stream_interval_statistics_multi_path_batch_from_transform_slices`
- `sample_interval_statistics_multi_path_batch_from_transform_slices`

Execution-mode helper contracts:

- `SufficientStatisticsStreamingChunkingSettings`
- `SufficientStatisticsIntervalStatisticsChunk`
- `SufficientStatisticsIntervalStatisticsPathChunk`
- `derive_sufficient_statistics_batch_path_seed`

Execution-mode policies:

- `SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY`
  - value: `fixed_max_intervals_per_chunk_with_ordered_interval_emission.v1`
- `SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY`
  - value:
    `path_seed_partitioned_from_root_seed_and_path_index_with_shared_substream_mixer.v1`

Multi-interval validation suite entrypoint:

- `run_sufficient_statistics_multi_interval_validation_suite`
  - validates short, medium, and long horizon scenario coverage
  - emits deterministic state-evolution invariant checks separately from
    tolerance-based statistical checks
  - produces machine-readable reports and optional persisted artifacts for CI comparability

Validation-suite contracts:

- `SufficientStatisticsValidationHorizonCategory`
- `SufficientStatisticsMultiIntervalValidationScenario`
- `SufficientStatisticsMultiIntervalValidationThresholds`
- `SufficientStatisticsMultiIntervalValidationCaseReport`
- `SufficientStatisticsMultiIntervalValidationReport`

Validation-suite report policies and schema versions:

- `SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_REPORT_SCHEMA_VERSION`
  - value: `validation.sufficient_statistics.multi_interval.report.v1`
- `SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_DETERMINISM_POLICY`
  - value: `stable_input_case_order_and_seed_replay_validation.v1`
- `SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_COVERAGE_POLICY`
  - value: `require_short_medium_and_long_horizon_scenarios.v1`
- `SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_OUTPUT_SCHEMA_METADATA_VERSION`
  - value:
    `validation.sufficient_statistics.multi_interval.output_schema_metadata.v1`
- `SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLDS_SCHEMA_VERSION`
  - value: `validation.sufficient_statistics.multi_interval.thresholds.v1`
- `SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLD_POLICY`
  - value:
    `deterministic_invariants_strict_and_interval_mean_tolerance_guard.v1`

Path metadata and diagnostics policies:

- `SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_POLICY`
  - value: `compatible_with_calibration_and_validation_artifact_workflows.v1`
- `SUFFICIENT_STATISTICS_RISK_PATH_ENVIRONMENT_METADATA_POLICY`
  - value: `exclude_environment_specific_metadata.v1`

The artifact-workflow policy and tags are compatibility and provenance markers.
They do not require filesystem writes and are valid for in-memory-only workflows.

Structured request validation errors:

- `SufficientStatisticsRiskRunRequestError`
  - `InvalidHorizonIntervalCount`
  - `InvalidIntervalDelta`
  - `InvalidModelInputs`
  - `InvalidIntervalFactors`
  - `IntervalDeltaFactorMismatch`
  - `HorizonExceedsAvailableIntervals`

Structured path validation errors:

- `SufficientStatisticsRiskPathError`
  - `InvalidIntervalDelta`
  - `InvalidSimulatedIntervalCount`
  - `RecordCountMismatch`
  - `InvalidInitialState`
  - `InvalidTerminalState`
  - `NonSequentialIntervalIndex`
  - `InvalidRecord`
  - `InvalidOutputSchemaMetadataField`
  - `InvalidProvenanceField`
  - `InvalidDiagnosticsField`

All contract payloads are `serde` serializable and validation-aware on
deserialization. Unknown fields and incomplete payloads are rejected.

## Python-facing contract

The Python-facing contract is in `janiform.risk`:

- `SufficientStatisticsRiskRunRequest`
- `SufficientStatisticsRiskPath`
- `SufficientStatisticsRiskPathOutputSchemaMetadata`
- `SufficientStatisticsRiskPathProvenance`
- `SufficientStatisticsRiskPathDiagnostics`
- `SufficientStatisticsRiskValidationFailure`
- `SufficientStatisticsRiskValidationError`

Validation and serialization helpers:

- `validate_sufficient_statistics_risk_run_request`
- `validate_sufficient_statistics_risk_path`
- `serialize_sufficient_statistics_risk_run_request`
- `deserialize_sufficient_statistics_risk_run_request`
- `serialize_sufficient_statistics_risk_path`
- `deserialize_sufficient_statistics_risk_path`

Python validation emits one structured failure with:

- `field_path`
- `message`

This allows downstream portfolio risk pipelines to route input issues without
string parsing.

## Interval factor policy

The interval-factor contract policy identifier is:

- `SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY`
  - value:
    `explicit_interval_aligned_s_h_and_news_intensity_inputs_without_implicit_resampling.v1`

The public request model requires explicit aligned vectors for `s_h` and one
of the following news-intensity input shapes:

- aggregate only
  - `nu_h`
- component-aware
  - `nu_h_known`
  - `nu_h_surprise`
- redundant aggregate plus components
  - `nu_h`
  - `nu_h_known`
  - `nu_h_surprise`

When component vectors are present, they must be aligned with `s_h`, finite,
non-negative, and satisfy
`nu_h = nu_h_known + nu_h_surprise` per interval within contract tolerance.

The transform-slice context forwards component-aware news-intensity payloads to
the affine oracle. The affine ODE still consumes only aggregate `total_nu_h`
for tractable gamma-moment terms while preserving known and surprise fields in
diagnostics and calibration wiring.

Implicit factor interpolation or resampling is not performed by contract
validation.

## Path Output Metadata And Diagnostics

`SufficientStatisticsRiskPath` includes deterministic machine-readable metadata:

- `schema_version`: `risk.sufficient_statistics.path.v1`
- `run_request_schema_version`: `risk.sufficient_statistics.run_request.v1`
- `output_schema_metadata`
  - `schema_version`: `risk.sufficient_statistics.path_output_schema_metadata.v1`
  - `required_path_fields`, `required_provenance_fields`,
    `required_diagnostics_fields`
- `provenance`
  - `schema_version`: `risk.sufficient_statistics.path_provenance.v1`
  - deterministic policy fields for interval factors, transform-slice sampling,
    boundary updates, return generation, and seed partitioning
  - `artifact_workflow_tags`: `["calibration", "validation"]`
- `diagnostics`
  - `schema_version`: `risk.sufficient_statistics.path_diagnostics.v1`
  - concise cumulative statistics and return totals for triage:
    `cumulative_a_plus`, `cumulative_a_minus`, `cumulative_n_plus`,
    `cumulative_n_minus`, `cumulative_trading_time_return`,
    `cumulative_open_jump_return`, `cumulative_closed_gap_return`,
    `cumulative_total_return`,
    `max_abs_total_return`

Python and Rust validation accept payloads without explicit metadata blocks and
canonicalize these values during validation. If caller-provided metadata blocks
are present, they must exactly match canonical deterministic values.

## Interval Delta Semantics

- `interval_delta` is the coarse-interval length `Delta` used by the risk-side
  oracle-solver interface (`SufficientStatisticsTransformSliceContext.delta` and
  downstream boundary-state updates).
- If omitted from run request payloads, it defaults to `1.0` for hourly
  compatibility.
- `interval_factors.interval_delta` is optional and defaults to `1.0`.
- The run request requires exact equality between `interval_delta` and
  `interval_factors.interval_delta`.
- Mismatched interval metadata fails validation with an explicit error.
  Implicit factor interpolation or resampling is not performed.
