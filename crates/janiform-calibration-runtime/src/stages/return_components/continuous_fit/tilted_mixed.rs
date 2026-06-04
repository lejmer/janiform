use std::f64::consts::PI;

use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};
use janiform_foundations::numeric::log_sum_exp_pair;
use janiform_model_kernels::nig::{NigBuySellDriverParameters, NigSharedPlusShiftConfiguration};
use janiform_model_kernels::risk_neutral::NigEsscherTiltDomain;

use super::super::contracts::{
    ReturnComponentsStageExactLikelihoodEvaluatorKind,
    ReturnComponentsStageExactLikelihoodFailureReason,
};
use super::exact_block::{ExactBlockInput, ExactLikelihoodFailureContext};
use super::likelihood::{ExactBlockLikelihoodEvaluation, ExactBlockStructuralFailure};

#[derive(Debug, Clone, Copy)]
struct TiltSelection {
    theta: f64,
    domain: NigEsscherTiltDomain,
}

#[derive(Debug, Clone, Copy)]
struct TiltedTailEnvelope {
    weighted_anchor_gamma: f64,
    weighted_decay_rate: f64,
}

#[derive(Debug, Clone, Copy)]
struct SimpsonPanel {
    lower: f64,
    upper: f64,
    lower_value: f64,
    midpoint_value: f64,
    upper_value: f64,
    whole_estimate: f64,
    tolerance: f64,
}

/// Maximum refined Simpson panels for one tilted mixed inversion block.
///
/// This is the deterministic work-budget contract for the exact likelihood
/// fallback: candidates whose certified density would require more panels are
/// rejected as numerically infeasible instead of consuming unbounded optimizer
/// time.
const TILTED_MIXED_ADAPTIVE_SIMPSON_REFINED_PANEL_BUDGET: usize = 65_536;
const TILTED_MIXED_ADAPTIVE_SIMPSON_REFINED_PANEL_BUDGET_AS_F64: f64 = 65_536.0;

pub(super) fn evaluate_tilted_mixed_exact_block(
    configuration: NigSharedPlusShiftConfiguration,
    driver_parameters: NigBuySellDriverParameters,
    block_input: ExactBlockInput,
    log_density_resolution: f64,
) -> Result<ExactBlockLikelihoodEvaluation, ExactBlockStructuralFailure> {
    let tilt_selection =
        select_tilt_theta(driver_parameters, block_input).map_err(|failure_reason| {
            ExactBlockStructuralFailure::new(
                block_input.block_member_index,
                failure_reason,
                ExactLikelihoodFailureContext {
                    evaluator_kind: Some(
                        ReturnComponentsStageExactLikelihoodEvaluatorKind::TiltedMixedInversion,
                    ),
                    observed_return: block_input.observed_return,
                    business_time_scale: block_input.business_time_scale,
                    sell_share_proxy: block_input.sell_share_proxy,
                    integrated_buy_intensity: block_input.integrated_buy_intensity,
                    integrated_sell_intensity: block_input.integrated_sell_intensity,
                    block_standard_deviation: Some(block_input.block_standard_deviation),
                    standardized_return: Some(block_input.standardized_return),
                    tilt_theta: None,
                    tilt_domain_lower_theta_exclusive: Some(
                        driver_parameters
                            .derive_return_composition_log_moment_domain()
                            .lower_theta_exclusive,
                    ),
                    tilt_domain_upper_theta_exclusive: Some(
                        driver_parameters
                            .derive_return_composition_log_moment_domain()
                            .upper_theta_exclusive,
                    ),
                    standardized_decay_scale: None,
                    integration_upper_bound: None,
                    log_density_resolution,
                },
            )
        })?;

    let prefactor_log = driver_parameters
        .evaluate_return_composition_log_moment(
            block_input.integrated_buy_intensity,
            block_input.integrated_sell_intensity,
            tilt_selection.theta,
        )
        .map_err(|_| {
            ExactBlockStructuralFailure::new(
                block_input.block_member_index,
                ReturnComponentsStageExactLikelihoodFailureReason::InvalidCharacteristicExponent,
                ExactLikelihoodFailureContext {
                    evaluator_kind: Some(
                        ReturnComponentsStageExactLikelihoodEvaluatorKind::TiltedMixedInversion,
                    ),
                    observed_return: block_input.observed_return,
                    business_time_scale: block_input.business_time_scale,
                    sell_share_proxy: block_input.sell_share_proxy,
                    integrated_buy_intensity: block_input.integrated_buy_intensity,
                    integrated_sell_intensity: block_input.integrated_sell_intensity,
                    block_standard_deviation: Some(block_input.block_standard_deviation),
                    standardized_return: Some(block_input.standardized_return),
                    tilt_theta: Some(tilt_selection.theta),
                    tilt_domain_lower_theta_exclusive: Some(
                        tilt_selection.domain.lower_theta_exclusive,
                    ),
                    tilt_domain_upper_theta_exclusive: Some(
                        tilt_selection.domain.upper_theta_exclusive,
                    ),
                    standardized_decay_scale: None,
                    integration_upper_bound: None,
                    log_density_resolution,
                },
            )
        })?
        - tilt_selection.theta * block_input.observed_return;

    let tail_envelope =
        derive_tilted_tail_envelope(configuration, block_input, tilt_selection.theta).map_err(
            |failure_reason| {
                ExactBlockStructuralFailure::new(
                    block_input.block_member_index,
                    failure_reason,
                    ExactLikelihoodFailureContext {
                        evaluator_kind: Some(
                            ReturnComponentsStageExactLikelihoodEvaluatorKind::TiltedMixedInversion,
                        ),
                        observed_return: block_input.observed_return,
                        business_time_scale: block_input.business_time_scale,
                        sell_share_proxy: block_input.sell_share_proxy,
                        integrated_buy_intensity: block_input.integrated_buy_intensity,
                        integrated_sell_intensity: block_input.integrated_sell_intensity,
                        block_standard_deviation: Some(block_input.block_standard_deviation),
                        standardized_return: Some(block_input.standardized_return),
                        tilt_theta: Some(tilt_selection.theta),
                        tilt_domain_lower_theta_exclusive: Some(
                            tilt_selection.domain.lower_theta_exclusive,
                        ),
                        tilt_domain_upper_theta_exclusive: Some(
                            tilt_selection.domain.upper_theta_exclusive,
                        ),
                        standardized_decay_scale: None,
                        integration_upper_bound: None,
                        log_density_resolution,
                    },
                )
            },
        )?;

    let integration_upper_bound =
        derive_integration_upper_bound(prefactor_log, tail_envelope, log_density_resolution)
            .ok_or_else(|| {
                ExactBlockStructuralFailure::new(
                    block_input.block_member_index,
                    ReturnComponentsStageExactLikelihoodFailureReason::IntegrationBoundOverflow,
                    ExactLikelihoodFailureContext {
                        evaluator_kind: Some(
                            ReturnComponentsStageExactLikelihoodEvaluatorKind::TiltedMixedInversion,
                        ),
                        observed_return: block_input.observed_return,
                        business_time_scale: block_input.business_time_scale,
                        sell_share_proxy: block_input.sell_share_proxy,
                        integrated_buy_intensity: block_input.integrated_buy_intensity,
                        integrated_sell_intensity: block_input.integrated_sell_intensity,
                        block_standard_deviation: Some(block_input.block_standard_deviation),
                        standardized_return: Some(block_input.standardized_return),
                        tilt_theta: Some(tilt_selection.theta),
                        tilt_domain_lower_theta_exclusive: Some(
                            tilt_selection.domain.lower_theta_exclusive,
                        ),
                        tilt_domain_upper_theta_exclusive: Some(
                            tilt_selection.domain.upper_theta_exclusive,
                        ),
                        standardized_decay_scale: Some(1.0 / tail_envelope.weighted_decay_rate),
                        integration_upper_bound: None,
                        log_density_resolution,
                    },
                )
            })?;

    if minimum_oscillation_panel_count_exceeds_budget(
        block_input.observed_return,
        integration_upper_bound,
    ) {
        return Err(ExactBlockStructuralFailure::new(
            block_input.block_member_index,
            ReturnComponentsStageExactLikelihoodFailureReason::IntegrationStepCountOverflow,
            ExactLikelihoodFailureContext {
                evaluator_kind: Some(
                    ReturnComponentsStageExactLikelihoodEvaluatorKind::TiltedMixedInversion,
                ),
                observed_return: block_input.observed_return,
                business_time_scale: block_input.business_time_scale,
                sell_share_proxy: block_input.sell_share_proxy,
                integrated_buy_intensity: block_input.integrated_buy_intensity,
                integrated_sell_intensity: block_input.integrated_sell_intensity,
                block_standard_deviation: Some(block_input.block_standard_deviation),
                standardized_return: Some(block_input.standardized_return),
                tilt_theta: Some(tilt_selection.theta),
                tilt_domain_lower_theta_exclusive: Some(
                    tilt_selection.domain.lower_theta_exclusive,
                ),
                tilt_domain_upper_theta_exclusive: Some(
                    tilt_selection.domain.upper_theta_exclusive,
                ),
                standardized_decay_scale: Some(1.0 / tail_envelope.weighted_decay_rate),
                integration_upper_bound: Some(integration_upper_bound),
                log_density_resolution,
            },
        ));
    }

    let integral_tail_upper_bound =
        integral_tail_upper_bound(tail_envelope, integration_upper_bound);

    let integral_tolerance = integral_tail_upper_bound.max(f64::MIN_POSITIVE);

    let integral_estimate = adaptive_simpson_integral_estimate(
        |frequency| {
            tilted_integrand(
                driver_parameters,
                block_input,
                tilt_selection.theta,
                frequency,
            )
        },
        integration_upper_bound,
        integral_tolerance,
    )
    .map_err(|failure_reason| {
        ExactBlockStructuralFailure::new(
            block_input.block_member_index,
            failure_reason,
            ExactLikelihoodFailureContext {
                evaluator_kind: Some(
                    ReturnComponentsStageExactLikelihoodEvaluatorKind::TiltedMixedInversion,
                ),
                observed_return: block_input.observed_return,
                business_time_scale: block_input.business_time_scale,
                sell_share_proxy: block_input.sell_share_proxy,
                integrated_buy_intensity: block_input.integrated_buy_intensity,
                integrated_sell_intensity: block_input.integrated_sell_intensity,
                block_standard_deviation: Some(block_input.block_standard_deviation),
                standardized_return: Some(block_input.standardized_return),
                tilt_theta: Some(tilt_selection.theta),
                tilt_domain_lower_theta_exclusive: Some(
                    tilt_selection.domain.lower_theta_exclusive,
                ),
                tilt_domain_upper_theta_exclusive: Some(
                    tilt_selection.domain.upper_theta_exclusive,
                ),
                standardized_decay_scale: Some(1.0 / tail_envelope.weighted_decay_rate),
                integration_upper_bound: Some(integration_upper_bound),
                log_density_resolution,
            },
        )
    })?;

    if !integral_estimate.estimate.is_finite() || integral_estimate.estimate <= 0.0 {
        return Err(ExactBlockStructuralFailure::new(
            block_input.block_member_index,
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidDensity,
            ExactLikelihoodFailureContext {
                evaluator_kind: Some(
                    ReturnComponentsStageExactLikelihoodEvaluatorKind::TiltedMixedInversion,
                ),
                observed_return: block_input.observed_return,
                business_time_scale: block_input.business_time_scale,
                sell_share_proxy: block_input.sell_share_proxy,
                integrated_buy_intensity: block_input.integrated_buy_intensity,
                integrated_sell_intensity: block_input.integrated_sell_intensity,
                block_standard_deviation: Some(block_input.block_standard_deviation),
                standardized_return: Some(block_input.standardized_return),
                tilt_theta: Some(tilt_selection.theta),
                tilt_domain_lower_theta_exclusive: Some(
                    tilt_selection.domain.lower_theta_exclusive,
                ),
                tilt_domain_upper_theta_exclusive: Some(
                    tilt_selection.domain.upper_theta_exclusive,
                ),
                standardized_decay_scale: Some(1.0 / tail_envelope.weighted_decay_rate),
                integration_upper_bound: Some(integration_upper_bound),
                log_density_resolution,
            },
        ));
    }

    let log_density_estimate = prefactor_log + integral_estimate.estimate.ln() - PI.ln();
    let log_density_error_upper_bound = log_density_error_upper_bound_from_logs(
        log_density_estimate,
        log_density_absolute_error_upper_bound_log(
            prefactor_log,
            integral_estimate.error_upper_bound,
            integral_tail_upper_bound,
        ),
    );

    Ok(ExactBlockLikelihoodEvaluation {
        evaluator_kind: ReturnComponentsStageExactLikelihoodEvaluatorKind::TiltedMixedInversion,
        log_density_estimate,
        log_density_error_upper_bound,
        tilt_theta: Some(tilt_selection.theta),
        tilt_domain_lower_theta_exclusive: Some(tilt_selection.domain.lower_theta_exclusive),
        tilt_domain_upper_theta_exclusive: Some(tilt_selection.domain.upper_theta_exclusive),
        block_standard_deviation: block_input.block_standard_deviation,
        standardized_return: block_input.standardized_return,
        standardized_decay_scale: Some(1.0 / tail_envelope.weighted_decay_rate),
        integration_upper_bound: Some(integration_upper_bound),
    })
}

#[derive(Debug, Clone, Copy)]
struct IntegralEstimate {
    estimate: f64,
    error_upper_bound: f64,
}

fn select_tilt_theta(
    driver_parameters: NigBuySellDriverParameters,
    block_input: ExactBlockInput,
) -> Result<TiltSelection, ReturnComponentsStageExactLikelihoodFailureReason> {
    let domain = driver_parameters.derive_return_composition_log_moment_domain();

    let Some(mut lower_theta) = domain.interior_lower_bound() else {
        return Err(
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidCharacteristicExponent,
        );
    };

    let Some(mut upper_theta) = domain.interior_upper_bound() else {
        return Err(
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidCharacteristicExponent,
        );
    };

    let theta = loop {
        let midpoint_theta = 0.5 * (lower_theta + upper_theta);
        if midpoint_theta <= lower_theta || midpoint_theta >= upper_theta {
            break 0.5 * (lower_theta + upper_theta);
        }

        let midpoint_residual = tilt_residual(driver_parameters, block_input, midpoint_theta)?;
        if midpoint_residual == 0.0 {
            break midpoint_theta;
        }

        if midpoint_residual.is_sign_negative() {
            lower_theta = midpoint_theta;
        } else {
            upper_theta = midpoint_theta;
        }
    };

    Ok(TiltSelection { theta, domain })
}

fn tilt_residual(
    driver_parameters: NigBuySellDriverParameters,
    block_input: ExactBlockInput,
    theta: f64,
) -> Result<f64, ReturnComponentsStageExactLikelihoodFailureReason> {
    let derivative = driver_parameters
        .differentiate_return_composition_log_moment(
            block_input.integrated_buy_intensity,
            block_input.integrated_sell_intensity,
            theta,
        )
        .map_err(|_| {
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidCharacteristicExponent
        })?;

    let residual = derivative - block_input.observed_return;
    if !residual.is_finite() {
        return Err(
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidCharacteristicExponent,
        );
    }

    Ok(residual)
}

fn derive_tilted_tail_envelope(
    configuration: NigSharedPlusShiftConfiguration,
    block_input: ExactBlockInput,
    theta: f64,
) -> Result<TiltedTailEnvelope, ReturnComponentsStageExactLikelihoodFailureReason> {
    let buy_anchor_margin =
        configuration.alpha * configuration.alpha - (configuration.beta_0 + theta).powi(2);

    let sell_anchor_margin = configuration.alpha * configuration.alpha
        - (configuration.derived_sell_beta() - theta).powi(2);

    if !buy_anchor_margin.is_finite()
        || buy_anchor_margin <= 0.0
        || !sell_anchor_margin.is_finite()
        || sell_anchor_margin <= 0.0
    {
        return Err(
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidCharacteristicExponent,
        );
    }

    let weighted_anchor_gamma = block_input.integrated_buy_intensity
        * configuration.delta
        * buy_anchor_margin.sqrt()
        + block_input.integrated_sell_intensity * configuration.delta * sell_anchor_margin.sqrt();

    let weighted_decay_rate = block_input.integrated_buy_intensity * configuration.delta
        + block_input.integrated_sell_intensity * configuration.delta;

    if !weighted_anchor_gamma.is_finite()
        || !weighted_decay_rate.is_finite()
        || weighted_decay_rate <= 0.0
    {
        return Err(
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidCharacteristicExponent,
        );
    }

    Ok(TiltedTailEnvelope {
        weighted_anchor_gamma,
        weighted_decay_rate,
    })
}

fn derive_integration_upper_bound(
    prefactor_log: f64,
    tail_envelope: TiltedTailEnvelope,
    log_density_resolution: f64,
) -> Option<f64> {
    if !prefactor_log.is_finite()
        || !tail_envelope.weighted_anchor_gamma.is_finite()
        || !tail_envelope.weighted_decay_rate.is_finite()
        || tail_envelope.weighted_decay_rate <= 0.0
        || !log_density_resolution.is_finite()
        || log_density_resolution <= 0.0
    {
        return None;
    }

    let target_log_tail_bound = log_density_resolution.ln();

    let required_upper_bound = (prefactor_log + tail_envelope.weighted_anchor_gamma
        - (PI * tail_envelope.weighted_decay_rate).ln()
        - target_log_tail_bound)
        / tail_envelope.weighted_decay_rate;

    Some(required_upper_bound.max(0.0).max(f64::MIN_POSITIVE))
}

fn integral_tail_upper_bound(
    tail_envelope: TiltedTailEnvelope,
    integration_upper_bound: f64,
) -> f64 {
    ((tail_envelope.weighted_anchor_gamma
        - tail_envelope.weighted_decay_rate * integration_upper_bound)
        .exp())
        / tail_envelope.weighted_decay_rate
}

fn minimum_oscillation_panel_count_exceeds_budget(
    observed_return: f64,
    integration_upper_bound: f64,
) -> bool {
    if !observed_return.is_finite()
        || !integration_upper_bound.is_finite()
        || integration_upper_bound < 0.0
    {
        return true;
    }

    let absolute_return = observed_return.abs();
    if absolute_return == 0.0 || integration_upper_bound == 0.0 {
        return false;
    }

    let minimum_panel_count = (integration_upper_bound * absolute_return / PI).ceil();

    !minimum_panel_count.is_finite()
        || minimum_panel_count > TILTED_MIXED_ADAPTIVE_SIMPSON_REFINED_PANEL_BUDGET_AS_F64
}

fn tilted_integrand(
    driver_parameters: NigBuySellDriverParameters,
    block_input: ExactBlockInput,
    theta: f64,
    frequency: f64,
) -> Result<f64, ReturnComponentsStageExactLikelihoodFailureReason> {
    let transformed_exponent = driver_parameters
        .transform_return_composition_characteristic_exponent_under_esscher_tilt(
            ComplexScalar::new(frequency, 0.0),
            block_input.integrated_buy_intensity,
            block_input.integrated_sell_intensity,
            theta,
        )
        .map_err(|_| {
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidCharacteristicExponent
        })?;
    if !transformed_exponent.is_finite() {
        return Err(
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidCharacteristicFunction,
        );
    }

    let characteristic_function = complex_exponential(transformed_exponent);
    if !characteristic_function.is_finite() {
        return Err(
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidCharacteristicFunction,
        );
    }

    let oscillation = complex_exponential(ComplexScalar::new(
        0.0,
        -frequency * block_input.observed_return,
    ));

    let integrand = complex_multiply(characteristic_function, oscillation).real;
    integrand
        .is_finite()
        .then_some(integrand)
        .ok_or(ReturnComponentsStageExactLikelihoodFailureReason::InvalidIntegrand)
}

fn adaptive_simpson_integral_estimate(
    integrand: impl Fn(f64) -> Result<f64, ReturnComponentsStageExactLikelihoodFailureReason>,
    integration_upper_bound: f64,
    integral_tolerance: f64,
) -> Result<IntegralEstimate, ReturnComponentsStageExactLikelihoodFailureReason> {
    let lower_value = integrand(0.0)?;

    let midpoint = 0.5 * integration_upper_bound;
    let midpoint_value = integrand(midpoint)?;

    let upper_value = integrand(integration_upper_bound)?;

    let whole_estimate = simpson_estimate(
        0.0,
        integration_upper_bound,
        lower_value,
        midpoint_value,
        upper_value,
    );

    let mut stack = vec![SimpsonPanel {
        lower: 0.0,
        upper: integration_upper_bound,
        lower_value,
        midpoint_value,
        upper_value,
        whole_estimate,
        tolerance: integral_tolerance,
    }];

    let mut total_estimate = 0.0;
    let mut total_error_upper_bound = 0.0;
    let mut refined_panel_count = 1usize;

    while let Some(panel) = stack.pop() {
        let midpoint = 0.5 * (panel.lower + panel.upper);

        let left_midpoint = 0.5 * (panel.lower + midpoint);
        let right_midpoint = 0.5 * (midpoint + panel.upper);

        if left_midpoint <= panel.lower
            || left_midpoint >= midpoint
            || right_midpoint <= midpoint
            || right_midpoint >= panel.upper
        {
            total_estimate += panel.whole_estimate;
            continue;
        }

        let left_midpoint_value = integrand(left_midpoint)?;
        let right_midpoint_value = integrand(right_midpoint)?;

        let left_estimate = simpson_estimate(
            panel.lower,
            midpoint,
            panel.lower_value,
            left_midpoint_value,
            panel.midpoint_value,
        );

        let right_estimate = simpson_estimate(
            midpoint,
            panel.upper,
            panel.midpoint_value,
            right_midpoint_value,
            panel.upper_value,
        );

        let refined_estimate = left_estimate + right_estimate;

        let error_upper_bound = ((refined_estimate - panel.whole_estimate) / 15.0).abs();
        if error_upper_bound <= panel.tolerance {
            total_estimate += refined_estimate + (refined_estimate - panel.whole_estimate) / 15.0;
            total_error_upper_bound += error_upper_bound;

            continue;
        }

        let split_tolerance = 0.5 * panel.tolerance;
        refined_panel_count = refined_panel_count
            .checked_add(1)
            .filter(|count| *count <= TILTED_MIXED_ADAPTIVE_SIMPSON_REFINED_PANEL_BUDGET)
            .ok_or(
                ReturnComponentsStageExactLikelihoodFailureReason::IntegrationStepCountOverflow,
            )?;

        stack.push(SimpsonPanel {
            lower: midpoint,
            upper: panel.upper,
            lower_value: panel.midpoint_value,
            midpoint_value: right_midpoint_value,
            upper_value: panel.upper_value,
            whole_estimate: right_estimate,
            tolerance: split_tolerance,
        });

        stack.push(SimpsonPanel {
            lower: panel.lower,
            upper: midpoint,
            lower_value: panel.lower_value,
            midpoint_value: left_midpoint_value,
            upper_value: panel.midpoint_value,
            whole_estimate: left_estimate,
            tolerance: split_tolerance,
        });
    }

    Ok(IntegralEstimate {
        estimate: total_estimate,
        error_upper_bound: total_error_upper_bound,
    })
}

fn simpson_estimate(
    lower: f64,
    upper: f64,
    lower_value: f64,
    midpoint_value: f64,
    upper_value: f64,
) -> f64 {
    (upper - lower) * (lower_value + 4.0 * midpoint_value + upper_value) / 6.0
}

fn log_density_absolute_error_upper_bound_log(
    prefactor_log: f64,
    quadrature_error_upper_bound: f64,
    tail_error_upper_bound: f64,
) -> f64 {
    let log_pi = PI.ln();
    let log_quadrature_error =
        prefactor_log + quadrature_error_upper_bound.max(f64::MIN_POSITIVE).ln() - log_pi;
    let log_tail_error =
        prefactor_log + tail_error_upper_bound.max(f64::MIN_POSITIVE).ln() - log_pi;

    log_sum_exp_pair(log_quadrature_error, log_tail_error)
}

fn log_density_error_upper_bound_from_logs(
    log_density_estimate: f64,
    log_density_absolute_error_upper_bound: f64,
) -> f64 {
    if !log_density_estimate.is_finite() || !log_density_absolute_error_upper_bound.is_finite() {
        return f64::INFINITY;
    }

    let log_relative_error = log_density_absolute_error_upper_bound - log_density_estimate;
    if log_relative_error >= 0.0 {
        return f64::INFINITY;
    }

    let relative_error = log_relative_error.exp();
    relative_error.ln_1p().max(-(1.0 - relative_error).ln())
}

#[cfg(test)]
mod tests {
    use super::{
        TILTED_MIXED_ADAPTIVE_SIMPSON_REFINED_PANEL_BUDGET_AS_F64,
        minimum_oscillation_panel_count_exceeds_budget,
    };

    #[test]
    fn oscillation_panel_precheck_rejects_candidates_beyond_budget() {
        let excessive_upper_bound = (TILTED_MIXED_ADAPTIVE_SIMPSON_REFINED_PANEL_BUDGET_AS_F64
            + 1.0)
            * std::f64::consts::PI;

        assert!(minimum_oscillation_panel_count_exceeds_budget(
            1.0,
            excessive_upper_bound
        ));
    }

    #[test]
    fn oscillation_panel_precheck_allows_zero_return() {
        assert!(!minimum_oscillation_panel_count_exceeds_budget(
            0.0,
            f64::MAX
        ));
    }
}
