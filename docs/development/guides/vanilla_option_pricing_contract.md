# Vanilla Option Pricing Contract

This guide defines the baseline v0.1 request and response contract for vanilla option pricing.

## Scope

The contract is for public pricing payloads only:

- request market and model inputs
- requested vanilla payoff configuration
- response price and non-engine-specific diagnostics

This contract intentionally excludes Fourier integration settings and solver internals.

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

Structured request validation errors:

- `VanillaOptionPricingRequestError`
  - `InvalidMarketInputs`
  - `InvalidBoundaryStateComponent`
  - `InvalidModelParameters`

Structured response validation errors:

- `VanillaOptionPricingResponseError`
  - `InvalidPrice`
  - `InvalidDiagnostics`

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
