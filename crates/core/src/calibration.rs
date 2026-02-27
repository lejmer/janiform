//! Calibration-side Hawkes entrypoints through shared provider and transition APIs.

use std::error::Error;
use std::fmt::{Display, Formatter};

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
use crate::pricing::{
    VanillaOptionPricingCalibrationHooks, VanillaOptionPricingCalibrationHooksError,
};
use crate::risk_neutral::{
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

#[cfg(test)]
mod tests {
    use crate::calibration::{
        OptionSurfaceCalibrationObservationError,
        build_option_surface_calibration_observation_from_pricing_hooks,
        evaluate_calibration_martingale_condition_under_esscher_tilt,
        precheck_hawkes_kernel_parameter_domain,
        precheck_structured_hawkes_kernel_parameter_domain,
        transform_nig_exponent_for_calibration_under_esscher_tilt,
    };
    use crate::hawkes::{
        HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION, HawkesKernelLoadings,
        HawkesKernelValidationStatus, HawkesStructuredAlphaParameterization,
        HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
    };
    use crate::nig::{ComplexScalar, NigParameters};
    use crate::pricing::VanillaOptionPricingCalibrationHooks;
    use crate::risk_neutral::{EsscherMartingaleConditionInput, NigEsscherTransformationInput};

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

    #[test]
    fn calibration_esscher_transform_adapter_delegates_to_shared_module() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("fixture parameters should construct");

        let transformed = transform_nig_exponent_for_calibration_under_esscher_tilt(
            NigEsscherTransformationInput::new(parameters, ComplexScalar::new(0.4, -0.2), -0.05),
        )
        .expect("Esscher transform should evaluate");

        assert!(transformed.transformed_exponent.is_finite());
    }

    #[test]
    fn calibration_martingale_adapter_delegates_to_shared_module() {
        let report = evaluate_calibration_martingale_condition_under_esscher_tilt(
            EsscherMartingaleConditionInput::new(ComplexScalar::new(0.12, 0.0), 0.1, 0.03),
        )
        .expect("martingale condition input should validate");

        assert!(report.is_satisfied);
        assert!(report.residual_norm <= report.tolerance);
    }

    #[test]
    fn calibration_consumer_observation_builder_accepts_valid_pricing_hooks() {
        let hooks = VanillaOptionPricingCalibrationHooks::try_new(
            8.5,
            -0.03,
            1.08,
            0.5,
            None,
            Some(false),
            Some(true),
            Some(true),
        )
        .expect("pricing hooks should construct");

        let observation = build_option_surface_calibration_observation_from_pricing_hooks(hooks)
            .expect("calibration observation conversion should succeed");

        assert_eq!(observation.option_price, hooks.option_price);
        assert_eq!(observation.log_moneyness, hooks.log_moneyness);
        assert_eq!(observation.forward_moneyness, hooks.forward_moneyness);
        assert_eq!(observation.maturity_interval, hooks.maturity_interval);
        assert_eq!(
            observation.integration_numeric_quality_is_poor,
            hooks.integration_numeric_quality_is_poor
        );
        assert_eq!(
            observation.integration_convergence_check_passed,
            hooks.integration_convergence_check_passed
        );
        assert_eq!(
            observation.integration_truncation_check_passed,
            hooks.integration_truncation_check_passed
        );
    }

    #[test]
    fn calibration_consumer_observation_builder_rejects_invalid_pricing_hooks() {
        let invalid_hooks = VanillaOptionPricingCalibrationHooks::new(
            8.5,
            -0.03,
            0.0,
            0.5,
            None,
            Some(false),
            Some(true),
            Some(true),
        );

        let error = build_option_surface_calibration_observation_from_pricing_hooks(invalid_hooks)
            .expect_err("invalid pricing hooks should fail calibration conversion");

        assert!(matches!(
            error,
            OptionSurfaceCalibrationObservationError::InvalidPricingCalibrationHooks { .. }
        ));
    }
}
