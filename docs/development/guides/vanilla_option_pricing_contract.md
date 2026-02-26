# Vanilla Option Pricing Contract

This guide defines the baseline v0.1 request and response contract for vanilla option pricing.

## Scope

The contract is for public pricing payloads only:

- request market and model inputs
- requested vanilla payoff configuration
- response price and non-engine-specific diagnostics

This contract intentionally excludes Fourier integration settings and solver internals.

## Fourier Integration Stability Controls

The Fourier engine in `janiform_core::pricing` now includes a structured integration stability policy:

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

## Rust contract

The canonical Rust types are in `janiform_core::pricing`:

- `VanillaOptionPayoffType`
- `VanillaOptionPayoffConfiguration`
- `VanillaOptionPricingMarketInputs`
- `VanillaOptionPricingModelParameters`
- `VanillaOptionPricingRequest`
- `VanillaOptionPricingDiagnostics`
- `VanillaOptionPricingResponse`

Validation entrypoints:

- `VanillaOptionPricingRequest::try_new`
- `VanillaOptionPricingResponse::try_new`
- `validate_pricing_martingale_condition_under_esscher_tilt`

Structured request validation errors:

- `VanillaOptionPricingRequestError`
  - `InvalidMarketInputs`
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

## Python-facing contract

The Python-facing contract is in `janiform.pricing`:

- `VanillaOptionPayoffType`
- `VanillaOptionPricingRequest` (typed dict)
- `VanillaOptionPricingResponse` (typed dict)
- `VanillaOptionPricingValidationFailure`
- `VanillaOptionPricingValidationError`

Validation and serialization helpers:

- `validate_vanilla_option_pricing_request`
- `validate_vanilla_option_pricing_response`
- `serialize_vanilla_option_pricing_request`
- `deserialize_vanilla_option_pricing_request`
- `serialize_vanilla_option_pricing_response`
- `deserialize_vanilla_option_pricing_response`

Python validation emits one structured failure with:

- `field_path`
- `message`

so callers can surface actionable input errors without string parsing.
