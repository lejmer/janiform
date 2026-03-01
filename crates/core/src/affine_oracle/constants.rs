pub const EXPONENTIAL_AFFINE_TRANSFORM_EQUATION_MAPPING_POLICY: &str =
    "compact_model_spec_section_8_named_component_mapping.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_DEFAULT_SOLVER_POLICY: &str =
    "fourth_order_runge_kutta_fixed_step.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_DOMAIN_GUARD_POLICY: &str =
    "enforce_d_dot_r_real_strict_upper_bound_at_runtime_checkpoints.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_BLOWUP_TERMINATION_POLICY: &str =
    "terminate_on_non_finite_state_or_guard_metric_without_clamping.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_NEWS_INTENSITY_USAGE_POLICY: &str =
    "gamma_moment_uses_total_news_intensity_components_are_diagnostic_inputs.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION: &str =
    "affine_oracle.integration_result.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION: &str =
    "affine_oracle.integration_metadata.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION: &str =
    "affine_oracle.integration_guard_failure.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION: &str =
    "affine_oracle.integration_telemetry.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "affine_oracle.integration_output_schema_metadata.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_RESULT_FIELDS: [&str; 6] = [
    "schema_version",
    "output_schema_metadata",
    "status",
    "metadata",
    "output",
    "telemetry",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_METADATA_FIELDS: [&str; 13] = [
    "schema_version",
    "integrator_method",
    "telemetry_mode",
    "status_flags",
    "news_intensity_usage_policy",
    "news_intensity",
    "configured_step_count",
    "completed_step_count",
    "step_size",
    "rhs_evaluation_count",
    "domain_guard_policy",
    "blowup_termination_policy",
    "guard_failure",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_GUARD_FAILURE_FIELDS: [&str; 6] = [
    "schema_version",
    "code",
    "step_index",
    "time",
    "d_dot_r",
    "d_dot_r_upper_bound",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_TELEMETRY_FIELDS: [&str; 9] = [
    "schema_version",
    "news_intensity",
    "checkpoint_count",
    "max_abs_c_value",
    "max_abs_d_component",
    "max_d_dot_r_real",
    "min_domain_margin",
    "guard_triggered",
    "guard_trigger",
];
