use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};
use janiform_foundations::numeric::square_root_machine_epsilon;
use janiform_model_kernels::nig::NigBuySellDriverParameters;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::characteristic::{
    AnchoredLocalTiltSolveError, LocalTiltResidualEvaluation, MARTINGALE_FIRST_LOG_MOMENT_SHIFT,
    build_local_tilt_bracket_with_search_domain_preferred_theta_and_residual_evaluator,
    resolve_local_martingale_tilt_search_domain,
};

use super::super::domain::{SpectralDomain, build_spectral_domain_from_anchor_span};
use super::super::maturity_law::SolvedMaturityLaw;
use super::super::{
    NodePriceSharedPlusShiftComplexSensitivity, SpectralPricingIntervalDiagnostics,
    SpectralThetaWarmStartKey, SpectralThetaWarmStartState, VanillaOptionPayoffType,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingRequest, VanillaOptionPricingRunError,
    install_pricing_parallelism,
};
use super::cumulant::{
    TerminalAffineCumulantArgument, TerminalAffineCumulantCache, TerminalAffineCumulantEvaluation,
};
use super::mean_reversion::{
    TerminalAffineMeanReversionPath, build_terminal_affine_mean_reversion_path,
};
use crate::progress::PricingProgressEvent;

#[derive(Debug, Clone, Copy)]
struct TerminalAffineFrequencyOutput {
    characteristic_value: ComplexScalar,
    sensitivity: Option<NodePriceSharedPlusShiftComplexSensitivity>,
}

#[derive(Debug, Clone, Copy)]
struct TerminalAffineThetaSolution {
    theta: f64,
    denominator: TerminalAffineCumulantEvaluation,
    buy_activity_premium_theta_derivative: f64,
    sell_activity_premium_theta_derivative: f64,
}

pub(super) struct TerminalAffineMaturityContext<'a> {
    pub(super) pricing_request: &'a VanillaOptionPricingRequest,
    pub(super) maturity_count: usize,
    pub(super) maturity_interval: f64,
    pub(super) spectral_domain: SpectralDomain,
    pub(super) mean_reversion_path: TerminalAffineMeanReversionPath,
    pub(super) interval_deltas: &'a [f64],
    pub(super) interval_factors: &'a [VanillaOptionPricingIntervalFactors],
    pub(super) driver_parameters: NigBuySellDriverParameters,
}

pub(in crate::spectral::pricer) fn solve_terminal_affine_maturity_law(
    pricing_request: &VanillaOptionPricingRequest,
    maturity_count: usize,
    maturity_interval: f64,
    strikes: &[f64],
    payoff_types: &[VanillaOptionPayoffType],
    collocation_node_count_override: Option<usize>,
    interval_deltas: &[f64],
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    driver_parameters: NigBuySellDriverParameters,
    include_node_price_sensitivities: bool,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    theta_warm_start_state: Option<&mut SpectralThetaWarmStartState>,
    theta_warm_start_key: SpectralThetaWarmStartKey,
    progress_observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<SolvedMaturityLaw, VanillaOptionPricingRunError> {
    validate_terminal_affine_inputs(maturity_count, interval_deltas, interval_factors)?;

    let mean_reversion_path = build_terminal_affine_mean_reversion_path(
        pricing_request,
        maturity_count,
        interval_deltas,
        interval_factors,
        driver_parameters,
    )?;

    let spectral_domain = build_spectral_domain_from_anchor_span(
        pricing_request,
        mean_reversion_path.terminal_anchor_value,
        mean_reversion_path.anchor_span,
        strikes,
        payoff_types,
        collocation_node_count_override,
    )?;

    let context = TerminalAffineMaturityContext {
        pricing_request,
        maturity_count,
        maturity_interval,
        spectral_domain,
        mean_reversion_path,
        interval_deltas,
        interval_factors,
        driver_parameters,
    };

    let preferred_theta = theta_warm_start_state
        .as_ref()
        .and_then(|state| state.theta_hint(theta_warm_start_key));

    let theta_solution =
        solve_terminal_affine_martingale_theta(&context, preferred_theta, progress_observer)?;
    
    let frequency_outputs = solve_terminal_affine_frequency_outputs(
        &context,
        theta_solution,
        include_node_price_sensitivities,
        pricing_thread_pool,
    )?;

    if let Some(theta_warm_start_state) = theta_warm_start_state {
        theta_warm_start_state.record_theta_solution(
            theta_warm_start_key,
            theta_solution.theta,
            theta_solution.buy_activity_premium_theta_derivative,
            theta_solution.sell_activity_premium_theta_derivative,
        );
    }

    let characteristic_function_sensitivities = include_node_price_sensitivities.then(|| {
        frequency_outputs
            .iter()
            .map(|output| {
                output
                    .sensitivity
                    .unwrap_or_else(NodePriceSharedPlusShiftComplexSensitivity::zero)
            })
            .collect::<Vec<_>>()
    });

    Ok(SolvedMaturityLaw {
        maturity_interval,
        terminal_anchor_value: context.mean_reversion_path.terminal_anchor_value,
        deviation_lower_bound: spectral_domain.deviation_lower_bound,
        deviation_upper_bound: spectral_domain.deviation_upper_bound,
        node_count: spectral_domain.node_count,
        observed_log_price: pricing_request
            .boundary_input
            .observed_boundary_state
            .observed_log_price,
        characteristic_function_values: frequency_outputs
            .into_iter()
            .map(|output| output.characteristic_value)
            .collect(),
        characteristic_function_sensitivities,
        interval_summaries: build_terminal_affine_interval_summaries(
            pricing_request,
            interval_deltas,
            maturity_count,
            theta_solution.theta,
            &context.mean_reversion_path,
        ),
    })
}

fn validate_terminal_affine_inputs(
    maturity_count: usize,
    interval_deltas: &[f64],
    interval_factors: &[VanillaOptionPricingIntervalFactors],
) -> Result<(), VanillaOptionPricingRunError> {
    if maturity_count == 0
        || maturity_count > interval_factors.len()
        || maturity_count > interval_deltas.len()
    {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "terminal affine maturity solve received inconsistent interval inputs"
                .to_string(),
        });
    }

    Ok(())
}

fn solve_terminal_affine_martingale_theta(
    context: &TerminalAffineMaturityContext<'_>,
    preferred_theta: Option<f64>,
    progress_observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<TerminalAffineThetaSolution, VanillaOptionPricingRunError> {
    let search_domain = resolve_local_martingale_tilt_search_domain(context.driver_parameters)
        .map_err(|source| {
            terminal_affine_measure_change_failure(
                context,
                terminal_affine_martingale_error_message(&source),
            )
        })?;

    let target_log_moment_exponent = (context.pricing_request.market_inputs.risk_free_rate
        - context.pricing_request.market_inputs.dividend_yield)
        * crate::time_scaling::intraday_year_fraction(context.maturity_interval);

    let mut cache = TerminalAffineCumulantCache::default();

    let (theta, _) =
        build_local_tilt_bracket_with_search_domain_preferred_theta_and_residual_evaluator(
            search_domain,
            target_log_moment_exponent,
            terminal_affine_martingale_root_tolerance(),
            preferred_theta,
            |theta| {
                evaluate_terminal_affine_martingale_residual(
                    context,
                    &mut cache,
                    theta,
                    target_log_moment_exponent,
                    progress_observer,
                )
            },
        )
        .map_err(|source| {
            terminal_affine_measure_change_failure(
                context,
                terminal_affine_martingale_error_message(&source),
            )
        })?;

    let denominator = cache
        .evaluate(context, TerminalAffineCumulantArgument::from_theta(theta))
        .map_err(|source| terminal_affine_measure_change_failure(context, source.to_string()))?;
    
    let numerator = cache
        .evaluate(
            context,
            TerminalAffineCumulantArgument::from_theta_shift(
                theta,
                MARTINGALE_FIRST_LOG_MOMENT_SHIFT,
            ),
        )
        .map_err(|source| terminal_affine_measure_change_failure(context, source.to_string()))?;

    let residual_theta_derivative = numerator
        .argument_derivative
        .subtract(denominator.argument_derivative)
        .real;
    if !residual_theta_derivative.is_finite() || residual_theta_derivative == 0.0 {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!(
                "terminal affine martingale theta sensitivity is undefined at theta={theta}"
            ),
        });
    }

    let buy_residual_derivative = numerator
        .buy_activity_premium_derivative
        .subtract(denominator.buy_activity_premium_derivative)
        .real;
    
    let sell_residual_derivative = numerator
        .sell_activity_premium_derivative
        .subtract(denominator.sell_activity_premium_derivative)
        .real;

    Ok(TerminalAffineThetaSolution {
        theta,
        denominator,
        buy_activity_premium_theta_derivative: -buy_residual_derivative / residual_theta_derivative,
        sell_activity_premium_theta_derivative: -sell_residual_derivative
            / residual_theta_derivative,
    })
}

fn terminal_affine_martingale_root_tolerance() -> f64 {
    square_root_machine_epsilon()
}

fn evaluate_terminal_affine_martingale_residual(
    context: &TerminalAffineMaturityContext<'_>,
    cache: &mut TerminalAffineCumulantCache,
    theta: f64,
    target_log_moment_exponent: f64,
    progress_observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<LocalTiltResidualEvaluation, AnchoredLocalTiltSolveError> {
    let numerator = cache
        .evaluate_with_progress(
            context,
            TerminalAffineCumulantArgument::from_theta_shift(
                theta,
                MARTINGALE_FIRST_LOG_MOMENT_SHIFT,
            ),
            progress_observer,
        )
        .map_err(|source| terminal_affine_tilt_error(theta, source))?;

    let denominator = cache
        .evaluate_with_progress(
            context,
            TerminalAffineCumulantArgument::from_theta(theta),
            progress_observer,
        )
        .map_err(|source| terminal_affine_tilt_error(theta, source))?;

    let log_moment = numerator.exponent.subtract(denominator.exponent).real;
    
    let log_moment = context
        .mean_reversion_path
        .terminal_deterministic_log_return
        + log_moment;

    if !log_moment.is_finite() {
        return Err(AnchoredLocalTiltSolveError::NonFiniteTransformInput {
            theta,
            value: ComplexScalar::new(log_moment, 0.0),
        });
    }

    Ok(LocalTiltResidualEvaluation {
        observed_log_moment_exponent: log_moment,
        residual: log_moment - target_log_moment_exponent,
    })
}

fn terminal_affine_measure_change_failure(
    context: &TerminalAffineMaturityContext<'_>,
    message: impl Into<String>,
) -> VanillaOptionPricingRunError {
    VanillaOptionPricingRunError::TerminalAffineMeasureChangeFailure {
        interval_index: context.pricing_request.boundary_input.interval_index,
        maturity_interval: context.maturity_interval,
        message: message.into(),
    }
}

fn terminal_affine_martingale_error_message(source: &AnchoredLocalTiltSolveError) -> String {
    match source {
        AnchoredLocalTiltSolveError::TransformEvaluationFailure { theta, message } => {
            format!("terminal affine cumulant could not be evaluated at theta={theta}: {message}")
        }
        AnchoredLocalTiltSolveError::NoAdmissibleLocalTilt {
            target_log_moment_exponent,
            lower_theta,
            upper_theta,
            lower_residual,
            upper_residual,
        } => format!(
            "terminal affine martingale solve found no admissible theta for target={target_log_moment_exponent} across [{lower_theta}, {upper_theta}] with residuals [{lower_residual}, {upper_residual}]"
        ),
        other => other.to_string(),
    }
}

fn terminal_affine_tilt_error(
    theta: f64,
    source: VanillaOptionPricingRunError,
) -> AnchoredLocalTiltSolveError {
    AnchoredLocalTiltSolveError::TransformEvaluationFailure {
        theta,
        message: source.to_string(),
    }
}

fn solve_terminal_affine_frequency_outputs(
    context: &TerminalAffineMaturityContext<'_>,
    theta_solution: TerminalAffineThetaSolution,
    include_node_price_sensitivities: bool,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
) -> Result<Vec<TerminalAffineFrequencyOutput>, VanillaOptionPricingRunError> {
    if let Some(pricing_thread_pool) = pricing_thread_pool {
        return install_pricing_parallelism(Some(pricing_thread_pool), || {
            (0..context.spectral_domain.node_count)
                .into_par_iter()
                .map(|frequency_index| {
                    solve_terminal_affine_frequency_output(
                        context,
                        theta_solution,
                        include_node_price_sensitivities,
                        frequency_index,
                    )
                })
                .collect()
        });
    }

    (0..context.spectral_domain.node_count)
        .map(|frequency_index| {
            solve_terminal_affine_frequency_output(
                context,
                theta_solution,
                include_node_price_sensitivities,
                frequency_index,
            )
        })
        .collect()
}

fn solve_terminal_affine_frequency_output(
    context: &TerminalAffineMaturityContext<'_>,
    theta_solution: TerminalAffineThetaSolution,
    include_node_price_sensitivities: bool,
    frequency_index: usize,
) -> Result<TerminalAffineFrequencyOutput, VanillaOptionPricingRunError> {
    let frequency = context.spectral_domain.basis_frequency(frequency_index);

    let mut cache = TerminalAffineCumulantCache::default();
    
    let numerator = cache.evaluate(
        context,
        TerminalAffineCumulantArgument::from_theta_and_frequency(theta_solution.theta, frequency),
    )?;
    
    let denominator = theta_solution.denominator;

    let stochastic_exponent = numerator.exponent.subtract(denominator.exponent);
    let stochastic_factor = complex_exponential(stochastic_exponent);

    let terminal_log_price_phase = complex_exponential(ComplexScalar::new(
        0.0,
        frequency * context.mean_reversion_path.terminal_deterministic_log_price,
    ));

    let characteristic_value = complex_multiply(terminal_log_price_phase, stochastic_factor);
    if !characteristic_value.is_finite() {
        return Err(
            VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction {
                numerator: characteristic_value,
                denominator: ComplexScalar::new(1.0, 0.0),
            },
        );
    }

    let sensitivity = include_node_price_sensitivities.then(|| {
        terminal_affine_frequency_sensitivity(
            characteristic_value,
            numerator,
            denominator,
            theta_solution,
        )
    });

    Ok(TerminalAffineFrequencyOutput {
        characteristic_value,
        sensitivity,
    })
}

fn terminal_affine_frequency_sensitivity(
    characteristic_value: ComplexScalar,
    numerator: TerminalAffineCumulantEvaluation,
    denominator: TerminalAffineCumulantEvaluation,
    theta_solution: TerminalAffineThetaSolution,
) -> NodePriceSharedPlusShiftComplexSensitivity {
    let numerator_minus_denominator_argument_derivative = numerator
        .argument_derivative
        .subtract(denominator.argument_derivative);
    
    let buy_log_derivative = numerator
        .buy_activity_premium_derivative
        .subtract(denominator.buy_activity_premium_derivative)
        .plus(
            numerator_minus_denominator_argument_derivative
                .scale(theta_solution.buy_activity_premium_theta_derivative),
        );
    
    let sell_log_derivative = numerator
        .sell_activity_premium_derivative
        .subtract(denominator.sell_activity_premium_derivative)
        .plus(
            numerator_minus_denominator_argument_derivative
                .scale(theta_solution.sell_activity_premium_theta_derivative),
        );

    NodePriceSharedPlusShiftComplexSensitivity {
        alpha_derivative: ComplexScalar::new(0.0, 0.0),
        beta_0_derivative: ComplexScalar::new(0.0, 0.0),
        delta_derivative: ComplexScalar::new(0.0, 0.0),
        delta_beta_derivative: ComplexScalar::new(0.0, 0.0),
        buy_activity_premium_derivative: complex_multiply(characteristic_value, buy_log_derivative),
        sell_activity_premium_derivative: complex_multiply(
            characteristic_value,
            sell_log_derivative,
        ),
    }
}

fn build_terminal_affine_interval_summaries(
    pricing_request: &VanillaOptionPricingRequest,
    interval_deltas: &[f64],
    maturity_count: usize,
    theta: f64,
    mean_reversion_path: &TerminalAffineMeanReversionPath,
) -> Vec<SpectralPricingIntervalDiagnostics> {
    (0..maturity_count)
        .map(|interval_offset| SpectralPricingIntervalDiagnostics {
            interval_index: pricing_request.boundary_input.interval_index + interval_offset,
            current_anchor_value: mean_reversion_path.interval_anchor_values_before
                [interval_offset],
            next_anchor_value: mean_reversion_path.interval_anchor_values_after[interval_offset],
            interval_delta: interval_deltas[interval_offset],
            expected_hawkes_boundary_state_before: mean_reversion_path
                .interval_hawkes_states_before[interval_offset],
            expected_hawkes_boundary_state_after: mean_reversion_path.interval_hawkes_states_after
                [interval_offset],
            minimum_solved_theta: theta,
            maximum_solved_theta: theta,
        })
        .collect()
}
