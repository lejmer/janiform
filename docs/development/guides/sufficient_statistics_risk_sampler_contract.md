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
- `SufficientStatisticsRiskPath`

Validation entrypoints:

- `SufficientStatisticsRiskRunRequest::try_new`
- `SufficientStatisticsRiskPath::try_new`

Return generation entrypoint:

- `generate_interval_return_from_sampled_statistics`
  - consumes sampled `A_plus` and `A_minus` with market-state-gated gap input
  - reuses shared return-decomposition sampling for sell-side sign handling
  - supports optional component breakdown through
    `SufficientStatisticsRiskReturnGenerationOptions`

Return generation policies:

- `SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY`
- `SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_BREAKDOWN_POLICY`
- `SUFFICIENT_STATISTICS_INTERVAL_RETURN_GAP_GATING_POLICY`

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

All contract payloads are `serde` serializable and validation-aware on
deserialization. Unknown fields and incomplete payloads are rejected.

## Python-facing contract

The Python-facing contract is in `janiform.risk`:

- `SufficientStatisticsRiskRunRequest`
- `SufficientStatisticsRiskPath`
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
    `explicit_interval_aligned_s_h_and_nu_h_vectors_without_implicit_resampling.v1`

The public request model requires explicit aligned vectors for `s_h` and `nu_h`.
Implicit factor interpolation or resampling is not performed by contract
validation.

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
