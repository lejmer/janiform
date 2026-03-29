# Vanilla Option Pricing Contract

This guide defines the baseline v0.1 request and response contract for vanilla option pricing.

## Scope

The contract is for public pricing payloads only:

- request market and model inputs
- requested vanilla payoff configuration
- anchored pricing boundary input with Hawkes and observed-state state
- optional pricing interval length `interval_delta` (`Delta`)
- response price and non-engine-specific diagnostics

This contract intentionally excludes Fourier integration settings and solver internals.
Executable bulk factor data for real pricing runs lives in a separate native-backed execution
artifact.

## Executable Pricing Input

The orchestrated pricing path uses:

- `VanillaOptionPricingExecutionInput`
- `HourlyFactorContract`
- `VanillaOptionPricingOptionalOutputSelection`
- `run_vanilla_option_pricing`

`VanillaOptionPricingExecutionInput.hourly_factor_contract` is a runtime artifact that keeps the
interval-aligned factor grid native-backed. The artifact carries the single-interval inputs needed
by the native runner across the requested pricing horizon:

- `s_h`
- `known_component_nu_h`
- `surprise_component_nu_h`

The execution path preserves known-scheduled and surprise news intensity separately and does not
require callers to collapse them into only one combined `nu_h`. Python no longer owns the canonical
pricing factor grid as a tuple of record dataclasses on the supported path.

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
- anchored recursion diagnostics (`AnchoredPricingRecursionDiagnostics`)
- calibration hooks (`VanillaOptionPricingCalibrationHooks`)

See `docs/development/guides/pricing_diagnostics_and_calibration_hooks_schema.md` for full schema
versions and required-field metadata.

## Rust contract

The canonical Rust types are in `janiform::pricing`:

- `AnchoredVanillaOptionPricingBoundaryInput`
- `AnchoredLocalTiltBoundaryStateSummary`
- `AnchoredLocalTiltDomainStatus`
- `AnchoredLocalTiltAdmissibilityStatus`
- `AnchoredLocalTiltSolveDiagnostics`
- `VanillaOptionPayoffType`
- `VanillaOptionPayoffConfiguration`
- `VanillaOptionPricingMarketInputs`
- `VanillaOptionPricingModelParameters`
- `VanillaOptionPricingRequest`
- `VanillaOptionPricingDiagnostics`
- `VanillaOptionPricingResponse`
- `VanillaOptionPricingExecutionInput`
- `VanillaOptionPricingOptionalOutputSelection`
- `VanillaOptionPricingRunResult`

Validation entrypoints:

- `AnchoredVanillaOptionPricingBoundaryInput::try_new`
- `VanillaOptionPricingRequest::try_new`
- `VanillaOptionPricingResponse::try_new`
- `VanillaOptionPricingExecutionInput::try_new`
- `solve_anchored_local_pricing_tilt`
- `run_vanilla_option_pricing`

Structured request validation errors:

- `VanillaOptionPricingRequestError`
  - `InvalidMarketInputs`
  - `InvalidIntervalDelta`
  - `InvalidBoundaryInput`
  - `BoundaryIntervalDeltaMismatch`
  - `InvalidModelParameters`

Anchored boundary-input validation errors:

- `AnchoredVanillaOptionPricingBoundaryInputError`
  - invalid Hawkes state component
  - invalid mean-reversion fitted parameters
  - invalid runtime anchor alignment
  - invalid observed-state payload
  - inconsistent interval index, current anchor, or observed anchor alignment

Structured response validation errors:

- `VanillaOptionPricingResponseError`
  - `InvalidPrice`
  - `InvalidDiagnostics`

Anchored local-tilt solve errors:

- `AnchoredLocalTiltSolveError`
  - `InvalidPricingRequest`
  - `InvalidReturnDecomposition`
  - `BuySideEsscherTransformFailure`
  - `SellSideEsscherTransformFailure`
  - `NonFiniteTransformInput`
  - `OutOfDomainStateRequest`
  - `InvalidTolerance`
  - `NoAdmissibleLocalTilt`

All request and response payloads are serializable and deserializable through `serde`.
Deserialization is validation-aware and rejects malformed or incomplete payloads.

## Anchored Boundary Input Semantics

`VanillaOptionPricingRequest` now carries `boundary_input` instead of a Hawkes-only
`boundary_state`. The canonical boundary payload augments the existing pricing-side Hawkes state
with the observed anchor-deviation state required by the anchored martingale condition:

- `interval_index`
- `hawkes_boundary_state`
- `mean_reversion_fitted_parameters`
- `mean_reversion_runtime_interval_anchors`
- `observed_boundary_state`

The request contract validates the interval-grid alignment implied by that state:

- `interval_delta` must match the runtime anchor interval at `interval_index`
- the observed boundary anchor must equal the current runtime anchor for that interval
- the anchor path is a runtime artifact, and the pricing recursion generates its `Q` anchor
  increments internally from the local tilted transform

The executable observed-return adjustment is derived from the boundary input as:

```latex
\[
(a_{h+1} - a_h) + (\phi_h - 1) D_h,
\qquad
\phi_h = e^{-\kappa \Delta_h}.
\]
```

## Anchored Local Pricing Measure

The canonical local pricing state is:

```latex
\[
Z_h = (Y_h, D_h),
\]
```

where `Y_h` is the existing Janiform boundary state used by the core transform and `D_h` is the
observed deviation from anchor at the boundary.

The public contract exposes one state-dependent local tilt solve through
`solve_anchored_local_pricing_tilt`. It keeps the existing Janiform core transform for `R_h`, but
enforces the anchored observed-return martingale condition:

```latex
\[
K_h^P(1 + \theta_h(Z_h) \mid Z_h) - K_h^P(\theta_h(Z_h) \mid Z_h)
=
(r - q)\Delta_h - (a_{h+1} - a_h) - (\phi_h - 1) D_h.
\]
```

The solve policy is explicit:

- derive the admissible `theta` interval from the exact buy-side and sell-side NIG Esscher domains
- evaluate the log-moment equation on the pure-imaginary axis as a real quantity instead of using
  complex-to-real coercion tolerances
- when multiple admissible roots exist, choose the admissible solution with the smallest
  `|theta|`

Successful solves emit `AnchoredLocalTiltSolveDiagnostics`, including:

- a boundary-state summary
- the target log-moment exponent
- the solved `theta`
- the residual and absolute residual
- domain and admissibility status
- the policy identifier

Current pricing-engine execution propagates European continuation values across the deviation state
on the canonical interval grid. The nested non-anchored case remains a special case inside that
engine when the observed-return adjustment is exactly zero for the local boundary state.

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
  should be carried by the shared `HourlyFactorContract` artifact.

## Python-facing contract

The Python-facing contract is in `janiform.pricing`:

- `AnchoredVanillaOptionPricingBoundaryInput` (frozen dataclass)
- `AnchoredLocalTiltBoundaryStateSummary` (frozen dataclass)
- `AnchoredLocalTiltDomainStatus`
- `AnchoredLocalTiltAdmissibilityStatus`
- `AnchoredLocalTiltSolveDiagnostics` (frozen dataclass)
- `MeanReversionAnchorModelKind`
- `MeanReversionAnchorModel` (frozen dataclass)
- `MeanReversionFittedParameters` (frozen dataclass)
- `MeanReversionRuntimeIntervalAnchors` (frozen dataclass)
- `MeanReversionBoundaryState` (frozen dataclass)
- `VanillaOptionPayoffType`
- `VanillaOptionPricingRequest` (frozen dataclass)
- `VanillaOptionPricingResponse` (frozen dataclass)
- `VanillaOptionPricingExecutionInput` (frozen dataclass)
- `VanillaOptionPricingOptionalOutputSelection` (frozen dataclass)
- `VanillaOptionPricingRunResult` (frozen dataclass)
- `VanillaOptionPricingValidationFailure`
- `VanillaOptionPricingValidationError`

Validation and serialization helpers:

- `validate_vanilla_option_pricing_request`
- `validate_vanilla_option_pricing_response`
- `validate_vanilla_option_pricing_run_result`
- `serialize_vanilla_option_pricing_request`
- `deserialize_vanilla_option_pricing_request`
- `serialize_vanilla_option_pricing_response`
- `deserialize_vanilla_option_pricing_response`
- `serialize_vanilla_option_pricing_run_result`
- `deserialize_vanilla_option_pricing_run_result`
- `run_vanilla_option_pricing`

The Python request contract mirrors the Rust request surface exactly. Anchored pricing payloads
must therefore be supplied through `boundary_input`, and local tilt diagnostics serialize with the
same deterministic anchored-state fields used by the Rust contract.

## High-Level Anchored Workflow

The shipped mean-reverting pricing workflow is anchored pricing.

High-level Python preparation uses `janiform.calibration.fit.model.JaniformModel.price_option(...)`
to derive interval-aligned pricing factors from a persisted fitted model. That high-level method
prepares the canonical `VanillaOptionPricingExecutionInput` and then calls
`run_vanilla_option_pricing`.

High-level boundary-input rules are:

- anchored pricing requires an explicit `observed_boundary_state: MeanReversionBoundaryState`
- the high-level API and CLI never invent missing observed anchor or deviation state
- CLI surface: `janiform price --observed-boundary-state-json PATH`

The returned `VanillaOptionPricingRunResult.run_diagnostics.anchored_recursion` payload is the
public diagnostics surface for anchored-engine numerical quality. It includes:

- deviation-grid metadata
- refinement deltas between the base and refined deviation grids
- per-interval diagnostics, including local tilt solve diagnostics, anchor alignment, and expected
  Hawkes-state carryover summaries

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

Anchored native-pricer versus simulation validation is governed separately from the default Rust
and Python test suites. Expensive Monte Carlo or seeded-simulation comparison workflows must run
only through explicitly invoked governance or benchmark paths.

Anchored governance entrypoints:

- `cargo test -p janiform-validation --test anchored_pricing_governance_fixture_contract`
- `cargo test -p janiform-validation --test anchored_pricing_native_vs_seeded_reference -- --ignored --nocapture`
