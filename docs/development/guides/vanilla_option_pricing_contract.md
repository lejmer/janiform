# Vanilla Option Pricing Contract

This guide defines the baseline v0.1 request and response contract for vanilla option pricing.

## Scope

The contract is for public pricing payloads only:

- request market and model inputs
- requested vanilla payoff configuration
- optional pricing interval length `interval_delta` (`Delta`)
- response price and non-engine-specific diagnostics

This contract intentionally excludes Fourier integration settings and solver internals.
Executable interval factors for real pricing runs live in a separate execution contract.

## Executable Pricing Input

The orchestrated pricing path uses:

- `VanillaOptionPricingExecutionInput`
- `VanillaOptionPricingIntervalFactors`
- `VanillaOptionPricingOptionalOutputSelection`
- `run_vanilla_option_pricing`

`VanillaOptionPricingIntervalFactors` carries the current interval inputs needed by the native
runner:

- `s_h`
- `nu_h_known`
- `nu_h_surprise`

The execution path preserves known-scheduled and surprise news intensity separately and does not
require callers to collapse them into only one combined `nu_h`.

## Fourier Integration Stability Controls

The Fourier engine in `janiform::pricing` now includes a structured integration stability policy:

- `FourierInversionIntegrationStabilityControls`
  - `convergence_absolute_tolerance`
  - `convergence_relative_tolerance`
  - `terminal_integrand_absolute_tolerance`
  - `terminal_integrand_ratio_tolerance`
  - `quality_gate` (`diagnostics_only` or `enforce_failure`)

Numeric quality checks are explicit:

- convergence check: full-resolution integral versus half-resolution integral
- truncation check: terminal-integrand absolute level and terminal-to-peak ratio

When quality is poor:

- `diagnostics_only`: pricing succeeds and emits quality diagnostics
- `enforce_failure`: pricing returns `FourierInversionPricingError::NumericQualityFailure`

This keeps numeric-quality diagnostics separate from business-level request and response validation errors.

## Optional Run Diagnostics And Calibration Hooks

Baseline response semantics remain unchanged.

Optional pricing run outputs are available through:

- `VanillaOptionPricingOptionalOutputSelection`
- `VanillaOptionPricingExecutionInput`
- `run_vanilla_option_pricing`
- `build_vanilla_option_pricing_run_result`
- `VanillaOptionPricingRunResult`

These optional outputs can include:

- transform diagnostics (`ExponentialAffineTransformIntegrationResult`)
- Fourier integration quality diagnostics (`FourierInversionDiagnostics`)
- calibration hooks (`VanillaOptionPricingCalibrationHooks`)

See `docs/development/guides/pricing_diagnostics_and_calibration_hooks_schema.md` for full schema
versions and required-field metadata.

## Rust contract

The canonical Rust types are in `janiform::pricing`:

- `VanillaOptionPayoffType`
- `VanillaOptionPayoffConfiguration`
- `VanillaOptionPricingMarketInputs`
- `VanillaOptionPricingModelParameters`
- `VanillaOptionPricingRequest`
- `VanillaOptionPricingDiagnostics`
- `VanillaOptionPricingResponse`
- `VanillaOptionPricingIntervalFactors`
- `VanillaOptionPricingExecutionInput`
- `VanillaOptionPricingOptionalOutputSelection`
- `VanillaOptionPricingRunResult`

Validation entrypoints:

- `VanillaOptionPricingRequest::try_new`
- `VanillaOptionPricingResponse::try_new`
- `VanillaOptionPricingIntervalFactors::try_new`
- `VanillaOptionPricingExecutionInput::try_new`
- `validate_pricing_martingale_condition_under_esscher_tilt`
- `run_vanilla_option_pricing`

Structured request validation errors:

- `VanillaOptionPricingRequestError`
  - `InvalidMarketInputs`
  - `InvalidIntervalDelta`
  - `InvalidBoundaryStateComponent`
  - `InvalidModelParameters`

Structured response validation errors:

- `VanillaOptionPricingResponseError`
  - `InvalidPrice`
  - `InvalidDiagnostics`

Risk-neutral martingale validation errors:

- `PricingMartingaleConditionValidationError`
  - `InvalidMarketInputs`
  - `InvalidReturnDecomposition`
  - `BuySideEsscherTransformFailure`
  - `SellSideEsscherTransformFailure`
  - `InvalidMartingaleInput`
  - `MartingaleConditionViolation`

All request and response payloads are serializable and deserializable through `serde`.
Deserialization is validation-aware and rejects malformed or incomplete payloads.

## Interval Delta Semantics

- `interval_delta` is the coarse-interval length `Delta` used by the pricing-side affine-oracle solve.
- If omitted from request payloads, it defaults to `1.0` (hourly intraday boundary default for v0.1).
- Non-hourly values are allowed in request validation and solver input construction when compatible interval factors are supplied by the caller.
- The hourly factor provider path remains hourly-only and rejects `interval_delta != 1.0` with a structured validation error.

## Hourly Factor Feeding Rules

- The shared hourly factor provider remains useful for hourly alignment and combined-factor loading.
- The executable pricing path needs explicit `s_h`, `nu_h_known`, and `nu_h_surprise` values so
  the solver bridge keeps corrected known-versus-surprise news semantics intact.
- When the caller already materialized split news components through
  `janiform.hourly_factors.materialize_hourly_news_intensity_factors`, those split interval values
  should be copied into `VanillaOptionPricingIntervalFactors`.

## Python-facing contract

The Python-facing contract is in `janiform.pricing`:

- `VanillaOptionPayoffType`
- `VanillaOptionPricingRequest` (frozen dataclass)
- `VanillaOptionPricingResponse` (frozen dataclass)
- `VanillaOptionPricingIntervalFactors` (frozen dataclass)
- `VanillaOptionPricingExecutionInput` (frozen dataclass)
- `VanillaOptionPricingOptionalOutputSelection` (frozen dataclass)
- `VanillaOptionPricingRunResult` (frozen dataclass)
- `VanillaOptionPricingValidationFailure`
- `VanillaOptionPricingValidationError`

Validation and serialization helpers:

- `validate_vanilla_option_pricing_request`
- `validate_vanilla_option_pricing_response`
- `validate_vanilla_option_pricing_execution_input`
- `validate_vanilla_option_pricing_run_result`
- `serialize_vanilla_option_pricing_request`
- `deserialize_vanilla_option_pricing_request`
- `serialize_vanilla_option_pricing_response`
- `deserialize_vanilla_option_pricing_response`
- `serialize_vanilla_option_pricing_execution_input`
- `deserialize_vanilla_option_pricing_execution_input`
- `serialize_vanilla_option_pricing_run_result`
- `deserialize_vanilla_option_pricing_run_result`
- `run_vanilla_option_pricing`

Python validation emits one structured failure with:

- `error_id`
- `workflow`
- `entrypoint`
- `field_path`
- `message`

See `docs/development/guides/python_error_taxonomy_and_exception_contract.md` for the shared
validation, execution, and integration taxonomy and the deterministic message format used by both
importable APIs and CLI surfaces.

## Benchmark Regression Fixture

The pricing benchmark and sanity-bound regression suite is fixture driven:

- Fixture file: `tests/fixtures/pricing/vanilla_option_pricing_benchmark_fixture.json`
- Fixture schema: `pricing.vanilla_option.benchmark_fixture.v1`
- Benchmark-result schema metadata:
  `pricing.vanilla_option.benchmark_result.v1`
- Tolerance-policy schema:
  `pricing.vanilla_option.benchmark_tolerance_policy.v1`

The fixture defines:

- baseline vanilla reference scenarios
- tolerance-based regression thresholds
- monotonicity and no-arbitrage guardrails
- explicit numeric-quality pathology probes

Rust regression entrypoint:

- `cargo test -p janiform --test pricing_benchmark_validation_regression`
