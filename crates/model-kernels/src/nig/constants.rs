pub const NIG_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION: &str =
    "validation.nig.parameter_report.v1";
pub const NIG_PARAMETER_VALIDATION_DETERMINISM_POLICY: &str =
    "stable_check_order_and_aggregated_failures.v1";
pub const NIG_INCREMENT_SAMPLING_POLICY: &str =
    "normal_inverse_gaussian_via_inverse_gaussian_subordinator_with_box_muller_standard_normal.v1";
pub const NIG_INCREMENT_NEAR_ZERO_SCALE_POLICY: &str =
    "zero_business_time_scale_returns_zero_without_random_draws.v1";
pub const NIG_SHARED_PLUS_SHIFT_CONFIGURATION_POLICY: &str =
    "buy_beta_equals_beta_0_and_sell_beta_equals_beta_0_plus_delta_beta.v1";
pub const NIG_TRADING_TIME_DRIVER_INTERFACE_POLICY: &str =
    "shared_buy_sell_driver_parameters_interface_for_pricing_and_simulation.v1";
pub const NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY: &str =
    "buy_side_uses_psi_plus_of_u_and_sell_side_uses_psi_minus_of_negative_u.v1";
pub const NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY: &str =
    "buy_side_increment_minus_sell_side_increment.v1";
pub const NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_REPORT_SCHEMA_VERSION: &str =
    "nig.characteristic_exponent.guardrail_report.v1";
pub const NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_DETERMINISM_POLICY: &str =
    "stable_check_order_with_explicit_threshold_metadata.v1";
pub const NIG_CHARACTERISTIC_EXPONENT_STABILITY_POLICY: &str =
    "gamma_minus_sqrt_term_uses_rationalized_difference_when_cancellation_risk_is_detected.v1";

pub(crate) const NIG_ALPHA_BETA_MARGIN_WARNING_RELATIVE_THRESHOLD: f64 = 1.0e-10;
pub(crate) const NIG_RADICAND_WARNING_RELATIVE_THRESHOLD: f64 = 1.0e-12;
pub(crate) const NIG_CANCELLATION_WARNING_THRESHOLD: f64 = 1.0e-10;
pub(crate) const NIG_COMPLEX_DIVISION_MIN_DENOMINATOR_NORM: f64 = 1.0e-24;
