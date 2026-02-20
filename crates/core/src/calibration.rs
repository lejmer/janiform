//! Calibration-side Hawkes entrypoints through shared provider and transition APIs.

use crate::calendar::ExchangeCalendarRegistry;
use crate::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelLoadings,
    HawkesKernelParameterValidationReport, HawkesKernelParameters, HawkesSeasonalityIntervalError,
    HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
    HawkesStructuredRhoSelfDominancePolicy, HawkesTransitionError, HawkesTransitionStep,
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

#[cfg(test)]
mod tests {
    use crate::calibration::{
        precheck_hawkes_kernel_parameter_domain, precheck_structured_hawkes_kernel_parameter_domain,
    };
    use crate::hawkes::{
        HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION, HawkesKernelLoadings,
        HawkesKernelValidationStatus, HawkesStructuredAlphaParameterization,
        HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
    };

    #[test]
    fn calibration_domain_precheck_report_shape_is_compatible_with_optimizer_consumers() {
        let report = precheck_hawkes_kernel_parameter_domain(
            0.85,
            1.25,
            HawkesKernelLoadings {
                plus_plus: 0.6,
                plus_minus: 0.1,
                minus_plus: 0.2,
                minus_minus: 0.7,
            },
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.05,
                minus_plus: 0.12,
                minus_minus: 0.0,
            },
        );

        assert_eq!(
            report.schema_version,
            HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION
        );
        assert_eq!(report.status, HawkesKernelValidationStatus::Pass);
        assert_eq!(report.summary.failed_check_count, 0);
        assert_eq!(
            report.summary.passed_check_count,
            report.summary.total_check_count
        );
        assert!(report.violations.is_empty());
    }

    #[test]
    fn calibration_structured_precheck_report_exposes_deterministic_failure_order() {
        let report = precheck_structured_hawkes_kernel_parameter_domain(
            0.0,
            -1.0,
            HawkesStructuredAlphaParameterization::new(0.0, -0.1, -0.1, -0.2),
            HawkesStructuredRhoParameterization::new(0.0, -0.2, -0.2, -0.3),
            Some(HawkesStructuredRhoSelfDominancePolicy::new(0.9)),
            -1.0,
        );

        assert_eq!(report.status, HawkesKernelValidationStatus::Fail);
        assert_eq!(
            report
                .violations
                .iter()
                .map(|violation| violation.field)
                .collect::<Vec<_>>(),
            vec![
                "lambda_bar",
                "beta",
                "structured_alpha.a_self",
                "structured_alpha.a_cross",
                "structured_alpha.delta_self",
                "structured_alpha.delta_cross",
                "alpha.plus_minus",
                "alpha.minus_plus",
                "alpha.minus_minus",
                "structured_rho.r_self",
                "structured_rho.r_cross",
                "structured_rho.delta_self_r",
                "structured_rho.delta_cross_r",
                "rho.plus_minus",
                "rho.minus_plus",
                "rho.minus_minus",
                "structured_rho.self_dominance_policy.minimum_self_to_cross_ratio",
                "stability.numeric_tolerance",
            ]
        );
    }
}
