# Affine Oracle Solver Diagnostics Schema

## Scope

This document defines the machine-readable diagnostics and optional telemetry payload emitted by
`solve_exponential_affine_transform(...)` and its pricing and risk adapters.

## Schema Versions

- Integration result schema: `affine_oracle.integration_result.v1`
- Integration metadata schema: `affine_oracle.integration_metadata.v1`
- Guard-failure schema: `affine_oracle.integration_guard_failure.v1`
- Integration telemetry schema: `affine_oracle.integration_telemetry.v1`
- Output-schema-metadata schema: `affine_oracle.integration_output_schema_metadata.v1`

## Top-Level Result Shape

`ExponentialAffineTransformIntegrationResult` includes:

- `schema_version`
- `output_schema_metadata`
- `status`
- `metadata`
- `output`
- `telemetry`

`status` remains the canonical termination state:

- `completed`
- `domain_guard_violation`
- `blowup_terminated`

## Integration Metadata

`metadata` includes:

- `schema_version`
- `integrator_method`
- `telemetry_mode`
- `status_flags`
- `configured_step_count`
- `completed_step_count`
- `step_size`
- `rhs_evaluation_count`
- `domain_guard_policy`
- `blowup_termination_policy`
- `guard_failure`

`status_flags` mirrors `status` with explicit booleans:

- `completed`
- `domain_guard_violation`
- `blowup_terminated`

`guard_failure` is present only on guarded termination and includes:

- `schema_version`
- `code`
- `step_index`
- `time`
- `d_dot_r`
- `d_dot_r_upper_bound`

## Telemetry Toggle

Telemetry is configured in `ExponentialAffineTransformSolverSettings.telemetry_mode`:

- `disabled` (default): `telemetry` is `null`
- `enabled`: solver emits `ExponentialAffineTransformIntegrationTelemetry`

Telemetry does not alter integration state transitions, guard behavior, or final outputs.

## Telemetry Payload

When enabled, `telemetry` includes:

- `schema_version`
- `checkpoint_count`
- `max_abs_c_value`
- `max_abs_d_component`
- `max_d_dot_r_real`
- `min_domain_margin`
- `guard_triggered`
- `guard_trigger`

`guard_trigger` mirrors the guard-failure payload and is populated only when a guard terminates the run.

## Output Schema Metadata

`output_schema_metadata` contains required-field lists for deterministic schema validation:

- `required_result_fields`
- `required_metadata_fields`
- `required_guard_failure_fields`
- `required_telemetry_fields`

This payload is environment-agnostic and contains no machine-specific paths.
