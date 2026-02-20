//! Calibration-side Hawkes entrypoints through shared provider and transition APIs.

use crate::calendar::ExchangeCalendarRegistry;
use crate::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelLoadings,
    HawkesKernelParameterValidationReport, HawkesKernelParameters, HawkesSeasonalityIntervalError,
    HawkesTransitionError, HawkesTransitionStep,
};
use crate::hourly_factor_provider::{
    HourlyFactorProviderError, HourlyFactorProviderRequest, provide_hourly_factor_contract,
};
use crate::hourly_factors::HourlyFactorContractOutput;

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

/// Project state to interval intensities for calibration objective evaluation.
pub fn project_hawkes_intensities_for_calibration_interval(
    state: HawkesExcitationState,
    parameters: HawkesKernelParameters,
    hourly_factor_contract: &HourlyFactorContractOutput,
    interval_index: usize,
) -> Result<HawkesIntensityProjection, HawkesSeasonalityIntervalError> {
    state.project_intensities_for_hourly_interval(
        parameters.lambda_bar,
        hourly_factor_contract,
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
