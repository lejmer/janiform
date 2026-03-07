# Pricing Diagnostics And Calibration Hooks Schema

## Scope

This document defines optional pricing run outputs used for diagnostics-heavy pricing workflows and
surface-calibration ingestion.

The baseline pricing response contract remains unchanged.
Optional run outputs are emitted only when explicitly selected.
The canonical public execution path now emits them through `run_vanilla_option_pricing`.

## Selection Toggle

`VanillaOptionPricingOptionalOutputSelection` controls optional output emission:

- `include_transform_diagnostics`
- `include_integration_quality_diagnostics`
- `include_calibration_hooks`

Default is all disabled.

The selection object is carried on `VanillaOptionPricingExecutionInput`.

## Schema Versions

- Run result schema: `pricing.vanilla_option.run_result.v1`
- Run diagnostics schema: `pricing.vanilla_option.run_diagnostics.v1`
- Calibration hooks schema: `pricing.vanilla_option.calibration_hooks.v1`
- Output-schema-metadata schema: `pricing.vanilla_option.run_output_schema_metadata.v1`

## Run Result Shape

`VanillaOptionPricingRunResult` includes:

- `schema_version`
- `output_schema_metadata`
- `response`
- `run_diagnostics`
- `calibration_hooks`

`response` is the existing `VanillaOptionPricingResponse`.

## Run Diagnostics Payload

When selected, `run_diagnostics` includes:

- `schema_version`
- `transform` (optional `ExponentialAffineTransformIntegrationResult`)
- `integration_quality` (optional `FourierInversionDiagnostics`)

At least one of `transform` or `integration_quality` must be present when `run_diagnostics` is
emitted.

## Calibration Hooks Payload

When selected, `calibration_hooks` includes:

- `schema_version`
- `hook_policy`
- `option_price`
- `log_moneyness`
- `forward_moneyness`
- `maturity_interval`
- `transform_status`
- `integration_numeric_quality_is_poor`
- `integration_convergence_check_passed`
- `integration_truncation_check_passed`

Hook policy:

- `derive_option_surface_inputs_from_pricing_response_and_optional_run_diagnostics.v1`

Calibration hooks are designed to be consumed by
`calibration::build_option_surface_calibration_observation_from_pricing_hooks`.

## Machine-Readable Required Fields

`output_schema_metadata` exposes required-field lists:

- `required_run_result_fields`
- `required_run_diagnostics_fields`
- `required_calibration_hooks_fields`

This keeps schema checks deterministic for calibration workflows and optional persisted artifact workflows.
