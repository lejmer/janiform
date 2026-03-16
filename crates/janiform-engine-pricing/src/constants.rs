pub const RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY: &str = "exp_of_trading_time_weighted_exponent_times_explicit_open_jump_and_closed_gap_characteristic_factors.v1";
pub const RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY: &str = "finite_argument_and_component_factors_with_valid_return_decomposition_and_finite_composition_result.v1";
pub const VANILLA_OPTION_PAYOFF_CONFIGURATION_POLICY: &str =
    "baseline_european_vanilla_call_and_put_payoff_type_selection_only.v1";
pub const VANILLA_OPTION_REQUEST_VALIDATION_POLICY: &str =
    "validate_market_inputs_boundary_state_and_model_parameter_domains_before_engine_dispatch.v1";
pub const VANILLA_OPTION_RESPONSE_VALIDATION_POLICY: &str =
    "validate_non_negative_price_and_finite_diagnostics_payload_before_response_publication.v1";
pub const PRICING_MARTINGALE_CONDITION_VALIDATION_POLICY: &str = "trading_time_log_moment_under_esscher_tilt_must_match_r_minus_q_maturity_target_within_tolerance.v1";
pub const FOURIER_INVERSION_ENGINE_POLICY: &str =
    "carr_madan_damped_call_inversion_with_put_call_parity_extension.v1";
pub const FOURIER_INVERSION_CONTOUR_POLICY: &str =
    "single_damping_factor_contour_shift_with_positive_alpha_requirement.v1";
pub const FOURIER_INVERSION_NUMERIC_SETTINGS_POLICY: &str =
    "bounded_frequency_quadrature_with_explicit_rule_selection_and_step_count.v1";
pub const FOURIER_INVERSION_INTEGRATION_STABILITY_POLICY: &str =
    "interval_tolerance_and_tail_truncation_quality_controls_with_tunable_quality_gate.v1";
pub const FOURIER_INVERSION_CONVERGENCE_CHECK_POLICY: &str =
    "half_resolution_integral_agreement_with_absolute_and_relative_tolerance.v1";
pub const FOURIER_INVERSION_TRUNCATION_CHECK_POLICY: &str =
    "terminal_integrand_absolute_and_peak_ratio_tail_checks.v1";
pub const FOURIER_INVERSION_NUMERIC_QUALITY_GATE_POLICY: &str =
    "diagnostics_only_or_explicit_quality_gate_failure_based_on_stability_controls.v1";
pub const FOURIER_INVERSION_DIAGNOSTICS_SCHEMA_VERSION: &str =
    "pricing.fourier_inversion.diagnostics.v1";
pub const VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION: &str =
    "pricing.vanilla_option.run_result.v1";
pub const VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION: &str =
    "pricing.vanilla_option.run_diagnostics.v1";
pub const VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_SCHEMA_VERSION: &str =
    "pricing.vanilla_option.calibration_hooks.v1";
pub const VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "pricing.vanilla_option.run_output_schema_metadata.v1";
pub const VANILLA_OPTION_CALIBRATION_HOOK_POLICY: &str =
    "derive_option_surface_inputs_from_pricing_response_and_optional_run_diagnostics.v1";
pub const DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA: f64 = 1.0;

pub(crate) const VANILLA_OPTION_PRICING_RUN_RESULT_REQUIRED_FIELDS: [&str; 5] = [
    "schema_version",
    "output_schema_metadata",
    "response",
    "run_diagnostics",
    "calibration_hooks",
];
pub(crate) const VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_REQUIRED_FIELDS: [&str; 3] =
    ["schema_version", "transform", "integration_quality"];
pub(crate) const VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_REQUIRED_FIELDS: [&str; 10] = [
    "schema_version",
    "hook_policy",
    "option_price",
    "log_moneyness",
    "forward_moneyness",
    "maturity_interval",
    "transform_status",
    "integration_numeric_quality_is_poor",
    "integration_convergence_check_passed",
    "integration_truncation_check_passed",
];

pub(crate) const DEFAULT_FOURIER_INVERSION_DAMPING_FACTOR: f64 = 1.5;
pub(crate) const DEFAULT_FOURIER_INVERSION_UPPER_INTEGRATION_BOUND: f64 = 150.0;
pub(crate) const DEFAULT_FOURIER_INVERSION_STEP_COUNT: usize = 4_096;
pub(crate) const DEFAULT_FOURIER_INVERSION_CONVERGENCE_ABSOLUTE_TOLERANCE: f64 = 1.0e-5;
pub(crate) const DEFAULT_FOURIER_INVERSION_CONVERGENCE_RELATIVE_TOLERANCE: f64 = 1.0e-4;
pub(crate) const DEFAULT_FOURIER_INVERSION_TERMINAL_INTEGRAND_ABSOLUTE_TOLERANCE: f64 = 5.0e-4;
pub(crate) const DEFAULT_FOURIER_INVERSION_TERMINAL_INTEGRAND_RATIO_TOLERANCE: f64 = 1.0e-2;
pub(crate) const FOURIER_NON_NEGATIVE_PRICE_CLAMP_TOLERANCE: f64 = 1.0e-12;
