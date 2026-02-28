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
- `news_intensity_usage_policy`
- `news_intensity`
- `configured_step_count`
- `completed_step_count`
- `step_size`
- `rhs_evaluation_count`
- `domain_guard_policy`
- `blowup_termination_policy`
- `guard_failure`

`news_intensity_usage_policy` currently resolves to:

- `gamma_moment_uses_total_news_intensity_components_are_diagnostic_inputs.v1`

`news_intensity` includes:

- `known_nu_h` (required)
- `surprise_nu_h` (required)

The affine `C'` gamma-moment term consumes aggregate intensity computed as
`known_nu_h + surprise_nu_h`. Component fields remain visible in metadata and
telemetry for diagnostics and calibration wiring.

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
- `news_intensity`
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

## Method Comparison Policy

Method-comparison checks are configured through
`ExponentialAffineTransformMethodComparisonTolerancePolicy`.

Schema version:

- `affine_oracle.method_comparison.tolerance_policy.v1`

Policy payload fields:

- `schema_version`
- `policy_version`
- `baseline_solver_settings`
- `alternative_solver_settings`
- `interval_tolerance_bands`
- `change_log`

Validation requirements:

- `baseline_solver_settings` and each `alternative_solver_settings` entry must use a strictly positive
  `step_count`
- `alternative_solver_settings` must be non-empty, unique, and different from baseline settings
- `interval_tolerance_bands` must be non-empty with strictly increasing `delta_upper_bound`
- each tolerance value in a band must be finite and strictly positive
- `change_log` must be non-empty and include the current `policy_version`

## Interval-Dependent Tolerance Guidance

Tolerance bands are selected by `Delta` using:

- `first_band_with_delta_upper_bound_greater_or_equal_to_delta.v1`

Each band defines:

- `interval_grid_label`
- `delta_upper_bound`
- `tolerance_window`

`tolerance_window` includes:

- `c_at_delta_absolute_tolerance`
- `d_component_absolute_tolerance`
- `transform_value_absolute_tolerance`

This gives explicit, machine-readable guidance for changing tolerance strictness as `Delta` moves
across supported grids.

## Method Comparison Report Schema

Method-comparison runs emit `ExponentialAffineTransformMethodComparisonReport`.

Schema versions:

- Report schema: `affine_oracle.method_comparison.report.v1`
- Output-schema-metadata schema: `affine_oracle.method_comparison.output_schema_metadata.v1`

Top-level report fields:

- `schema_version`
- `output_schema_metadata`
- `policy_schema_version`
- `policy_version`
- `agreement_policy`
- `interval_selection_policy`
- `reference_fixture_policy`
- `input_delta`
- `tolerance_band`
- `baseline_run`
- `alternative_runs`
- `all_checks_passed`

Agreement policy:

- `require_completed_status_and_absolute_tolerance_agreement.v1`

Reference-fixture relationship:

- `method_comparison_checks_are_complementary_to_reference_fixtures.v1`

`output_schema_metadata` includes machine-readable required-field lists for:

- report fields
- tolerance band fields
- tolerance window fields
- run summary fields
- alternative run fields
- output difference fields
