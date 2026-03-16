//! Calibration-stage orchestration entrypoints and shared adapters.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::calendar::ExchangeCalendarRegistry;
use crate::hourly_factors::{
    HourlyFactorContractOutput, HourlyFactorProviderError, HourlyFactorProviderRequest,
    provide_hourly_factor_contract,
};
use crate::pricing::{
    VanillaOptionPricingCalibrationHooks, VanillaOptionPricingCalibrationHooksError,
};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelLoadings,
    HawkesKernelParameterValidationReport, HawkesKernelParameters, HawkesSeasonalityIntervalError,
    HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
    HawkesStructuredRhoSelfDominancePolicy, HawkesTransitionError, HawkesTransitionStep,
};
use janiform_model_kernels::risk_neutral::{
    EsscherMartingaleConditionError, EsscherMartingaleConditionInput,
    EsscherMartingaleConditionReport, EsscherTransformError, NigEsscherTransformationInput,
    NigEsscherTransformedExponent, evaluate_esscher_martingale_condition,
    transform_nig_characteristic_exponent_under_esscher_tilt,
};

/// Resolve interval-aligned hourly factors for the calibration path.
pub fn load_hourly_factor_contract_for_calibration(
    calendar_registry: &ExchangeCalendarRegistry,
    provider_request: HourlyFactorProviderRequest<'_>,
) -> Result<HourlyFactorContractOutput, HourlyFactorProviderError> {
    provide_hourly_factor_contract(calendar_registry, provider_request)
}

/// Run pure Hawkes kernel-domain checks before expensive calibration loops.
pub fn precheck_hawkes_kernel_parameter_domain(
    lambda_bar: f64,
    beta: f64,
    alpha: HawkesKernelLoadings,
    rho: HawkesKernelLoadings,
) -> HawkesKernelParameterValidationReport {
    HawkesKernelParameters::validate_domain(lambda_bar, beta, alpha, rho)
}

/// Run structured base-plus-delta checks before calibration loops over structured parameters.
pub fn precheck_structured_hawkes_kernel_parameter_domain(
    lambda_bar: f64,
    beta: f64,
    structured_alpha: HawkesStructuredAlphaParameterization,
    structured_rho: HawkesStructuredRhoParameterization,
    self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
    stability_numeric_tolerance: f64,
) -> HawkesKernelParameterValidationReport {
    HawkesKernelParameters::validate_structured_domain_with_stability_tolerance(
        lambda_bar,
        beta,
        structured_alpha,
        structured_rho,
        self_dominance_policy,
        stability_numeric_tolerance,
    )
}

/// Project state to interval intensities for calibration objective evaluation.
pub fn project_hawkes_intensities_for_calibration_interval(
    state: HawkesExcitationState,
    parameters: HawkesKernelParameters,
    hourly_factor_contract: &HourlyFactorContractOutput,
    interval_index: usize,
) -> Result<HawkesIntensityProjection, HawkesSeasonalityIntervalError> {
    state.project_intensities_for_hourly_interval(
        parameters.lambda_bar,
        &hourly_factor_contract.s_h,
        interval_index,
    )
}

/// Apply one calibration-path Hawkes state transition using canonical call order.
///
/// Call order is delegated to `HawkesExcitationState::apply_transition_step`:
/// drift, then optional endogenous jump, then optional exogenous jump.
pub fn evolve_hawkes_state_for_calibration_step(
    state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    transition_step: HawkesTransitionStep,
) -> Result<HawkesExcitationState, HawkesTransitionError> {
    state.apply_transition_step(kernel_parameters, transition_step)
}

/// Evaluate one Esscher-transformed NIG exponent for calibration-side objectives.
pub fn transform_nig_exponent_for_calibration_under_esscher_tilt(
    input: NigEsscherTransformationInput,
) -> Result<NigEsscherTransformedExponent, EsscherTransformError> {
    transform_nig_characteristic_exponent_under_esscher_tilt(input)
}

/// Evaluate one Esscher martingale-condition residual for calibration-side objectives.
pub fn evaluate_calibration_martingale_condition_under_esscher_tilt(
    input: EsscherMartingaleConditionInput,
) -> Result<EsscherMartingaleConditionReport, EsscherMartingaleConditionError> {
    evaluate_esscher_martingale_condition(input)
}

/// Canonical calibration observation distilled from pricing calibration hooks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OptionSurfaceCalibrationObservation {
    pub option_price: f64,
    pub log_moneyness: f64,
    pub forward_moneyness: f64,
    pub maturity_interval: f64,
    pub integration_numeric_quality_is_poor: Option<bool>,
    pub integration_convergence_check_passed: Option<bool>,
    pub integration_truncation_check_passed: Option<bool>,
}

/// Consume one pricing calibration hook payload into the calibration observation shape.
pub fn build_option_surface_calibration_observation_from_pricing_hooks(
    hooks: VanillaOptionPricingCalibrationHooks,
) -> Result<OptionSurfaceCalibrationObservation, OptionSurfaceCalibrationObservationError> {
    hooks.validate().map_err(|source| {
        OptionSurfaceCalibrationObservationError::InvalidPricingCalibrationHooks { source }
    })?;

    Ok(OptionSurfaceCalibrationObservation {
        option_price: hooks.option_price,
        log_moneyness: hooks.log_moneyness,
        forward_moneyness: hooks.forward_moneyness,
        maturity_interval: hooks.maturity_interval,
        integration_numeric_quality_is_poor: hooks.integration_numeric_quality_is_poor,
        integration_convergence_check_passed: hooks.integration_convergence_check_passed,
        integration_truncation_check_passed: hooks.integration_truncation_check_passed,
    })
}

/// Structured failure modes for pricing-hook calibration observation conversion.
#[derive(Debug, Clone, PartialEq)]
pub enum OptionSurfaceCalibrationObservationError {
    InvalidPricingCalibrationHooks {
        source: VanillaOptionPricingCalibrationHooksError,
    },
}

impl Display for OptionSurfaceCalibrationObservationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPricingCalibrationHooks { source } => {
                write!(
                    formatter,
                    "pricing calibration hooks payload is invalid for calibration consumption: {source}"
                )
            }
        }
    }
}

impl Error for OptionSurfaceCalibrationObservationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPricingCalibrationHooks { source } => Some(source),
        }
    }
}
