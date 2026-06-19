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
    NodePriceSharedPlusShiftComplexSensitivity, SpectralThetaWarmStartKey,
    SpectralThetaWarmStartState, VanillaOptionPayoffType, VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingRequest, VanillaOptionPricingRunError, install_pricing_parallelism,
};
use super::cumulant::{
    TerminalAffineCumulantArgument, TerminalAffineCumulantCache, TerminalAffineCumulantEvaluation,
};
use super::diagnostics::{
    build_terminal_affine_interval_summaries, build_terminal_affine_measure_change_diagnostics,
};
use super::mean_reversion::{
    TerminalAffineMeanReversionPath, build_terminal_affine_mean_reversion_path,
};
use super::moments::{
    expected_log_return_from_theta_solution, log_expected_price_ratio_from_theta_solution,
    terminal_affine_first_log_moment,
};
use crate::progress::PricingProgressEvent;

#[derive(Debug, Clone, Copy)]
struct TerminalAffineFrequencyOutput {
    characteristic_value: ComplexScalar,
    sensitivity: Option<NodePriceSharedPlusShiftComplexSensitivity>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum TerminalAffineMeasurePolicy {
    Physical,
    RiskNeutralMartingale,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TerminalAffineThetaSolution {
    pub(super) theta: f64,
    pub(super) denominator: TerminalAffineCumulantEvaluation,
    pub(super) target_log_moment_exponent: f64,
    pub(super) observed_log_moment_exponent: f64,
    pub(super) martingale_residual: f64,
    pub(super) martingale_tolerance: f64,
    buy_activity_premium_theta_derivative: f64,
    sell_activity_premium_theta_derivative: f64,
}

pub(super) struct TerminalAffineMaturityContext<'a> {
    pub(super) pricing_request: &'a VanillaOptionPricingRequest,
    pub(super) maturity_count: usize,
    pub(super) maturity_interval: f64,
    pub(super) mean_reversion_path: TerminalAffineMeanReversionPath,
    pub(super) interval_deltas: &'a [f64],
    pub(super) interval_factor_grids: &'a [VanillaOptionPricingIntervalFactors],
    pub(super) driver_parameters: NigBuySellDriverParameters,
}

pub(crate) struct TerminalAffineExplicitLogPriceDomain {
    pub(crate) lower_bound: f64,
    pub(crate) upper_bound: f64,
    pub(crate) node_count: usize,
}

pub(crate) enum TerminalAffineSpectralDomainInput<'a> {
    PayoffAnchored {
        strikes: &'a [f64],
        payoff_types: &'a [VanillaOptionPayoffType],
        collocation_node_count_override: Option<usize>,
    },
    ExplicitLogPrice(TerminalAffineExplicitLogPriceDomain),
}

pub(crate) struct TerminalAffineMaturitySolveInput<'a> {
    pub(crate) pricing_request: &'a VanillaOptionPricingRequest,
    pub(crate) maturity_count: usize,
    pub(crate) maturity_interval: f64,
    pub(crate) spectral_domain_input: TerminalAffineSpectralDomainInput<'a>,
    pub(crate) interval_deltas: &'a [f64],
    pub(crate) interval_factor_grids: &'a [VanillaOptionPricingIntervalFactors],
    pub(crate) driver_parameters: NigBuySellDriverParameters,
    pub(crate) measure_policy: TerminalAffineMeasurePolicy,
    pub(crate) include_node_price_sensitivities: bool,
    pub(crate) pricing_thread_pool: Option<&'a rayon::ThreadPool>,
    pub(crate) theta_warm_start_key: SpectralThetaWarmStartKey,
}

pub(crate) struct TerminalAffineExpectedLogReturnInput<'a> {
    pub(crate) pricing_request: &'a VanillaOptionPricingRequest,
    pub(crate) maturity_count: usize,
    pub(crate) maturity_interval: f64,
    pub(crate) interval_deltas: &'a [f64],
    pub(crate) interval_factor_grids: &'a [VanillaOptionPricingIntervalFactors],
    pub(crate) driver_parameters: NigBuySellDriverParameters,
    pub(crate) measure_policy: TerminalAffineMeasurePolicy,
    pub(crate) theta_warm_start_key: SpectralThetaWarmStartKey,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TerminalAffineExpectedLogReturn {
    pub(crate) maturity_interval: f64,
    pub(crate) expected_log_return: f64,
    pub(crate) log_expected_price_ratio: f64,
}

pub(crate) fn solve_terminal_affine_maturity_law(
    solve_input: TerminalAffineMaturitySolveInput<'_>,
    theta_warm_start_state: Option<&mut SpectralThetaWarmStartState>,
    progress_observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<SolvedMaturityLaw, VanillaOptionPricingRunError> {
    validate_terminal_affine_inputs(
        solve_input.maturity_count,
        solve_input.interval_deltas,
        solve_input.interval_factor_grids,
    )?;

    let mean_reversion_path = build_terminal_affine_mean_reversion_path(
        solve_input.pricing_request,
        solve_input.maturity_count,
        solve_input.interval_deltas,
        solve_input.interval_factor_grids,
        solve_input.driver_parameters,
    )?;

    let spectral_domain = resolve_terminal_affine_spectral_domain(
        solve_input.pricing_request,
        &mean_reversion_path,
        solve_input.spectral_domain_input,
    )?;

    let context = TerminalAffineMaturityContext {
        pricing_request: solve_input.pricing_request,
        maturity_count: solve_input.maturity_count,
        maturity_interval: solve_input.maturity_interval,
        mean_reversion_path,
        interval_deltas: solve_input.interval_deltas,
        interval_factor_grids: solve_input.interval_factor_grids,
        driver_parameters: solve_input.driver_parameters,
    };

    let preferred_theta = theta_warm_start_state
        .as_ref()
        .and_then(|state| state.theta_hint(solve_input.theta_warm_start_key));

    let theta_solution = solve_terminal_affine_measure_theta(
        &context,
        solve_input.measure_policy,
        preferred_theta,
        progress_observer,
    )?;

    let expected_log_return = expected_log_return_from_theta_solution(&context, theta_solution)?;

    let frequency_outputs = solve_terminal_affine_frequency_outputs(
        &context,
        spectral_domain,
        theta_solution,
        solve_input.include_node_price_sensitivities,
        solve_input.pricing_thread_pool,
    )?;

    if solve_input.measure_policy == TerminalAffineMeasurePolicy::RiskNeutralMartingale
        && let Some(theta_warm_start_state) = theta_warm_start_state
    {
        theta_warm_start_state.record_theta_solution(
            solve_input.theta_warm_start_key,
            theta_solution.theta,
            theta_solution.buy_activity_premium_theta_derivative,
            theta_solution.sell_activity_premium_theta_derivative,
        );
    }

    let characteristic_function_sensitivities =
        solve_input.include_node_price_sensitivities.then(|| {
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
        maturity_interval: solve_input.maturity_interval,
        terminal_anchor_value: context.mean_reversion_path.terminal_anchor_value,
        deviation_lower_bound: spectral_domain.deviation_lower_bound,
        deviation_upper_bound: spectral_domain.deviation_upper_bound,
        node_count: spectral_domain.node_count,
        observed_log_price: solve_input
            .pricing_request
            .boundary_input
            .observed_boundary_state
            .observed_log_price,
        expected_log_return,
        characteristic_function_values: frequency_outputs
            .into_iter()
            .map(|output| output.characteristic_value)
            .collect(),
        characteristic_function_sensitivities,
        terminal_affine_measure_change: match solve_input.measure_policy {
            TerminalAffineMeasurePolicy::Physical => None,
            TerminalAffineMeasurePolicy::RiskNeutralMartingale => Some(
                build_terminal_affine_measure_change_diagnostics(&context, theta_solution)?,
            ),
        },
        interval_summaries: build_terminal_affine_interval_summaries(
            solve_input.pricing_request,
            solve_input.interval_deltas,
            solve_input.maturity_count,
            theta_solution.theta,
            &context.mean_reversion_path,
        ),
    })
}

pub(crate) fn solve_terminal_affine_expected_log_return(
    solve_input: TerminalAffineExpectedLogReturnInput<'_>,
    theta_warm_start_state: Option<&mut SpectralThetaWarmStartState>,
    progress_observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<TerminalAffineExpectedLogReturn, VanillaOptionPricingRunError> {
    validate_terminal_affine_inputs(
        solve_input.maturity_count,
        solve_input.interval_deltas,
        solve_input.interval_factor_grids,
    )?;

    let mean_reversion_path = build_terminal_affine_mean_reversion_path(
        solve_input.pricing_request,
        solve_input.maturity_count,
        solve_input.interval_deltas,
        solve_input.interval_factor_grids,
        solve_input.driver_parameters,
    )?;

    let context = TerminalAffineMaturityContext {
        pricing_request: solve_input.pricing_request,
        maturity_count: solve_input.maturity_count,
        maturity_interval: solve_input.maturity_interval,
        mean_reversion_path,
        interval_deltas: solve_input.interval_deltas,
        interval_factor_grids: solve_input.interval_factor_grids,
        driver_parameters: solve_input.driver_parameters,
    };

    let preferred_theta = theta_warm_start_state
        .as_ref()
        .and_then(|state| state.theta_hint(solve_input.theta_warm_start_key));

    let theta_solution = solve_terminal_affine_measure_theta(
        &context,
        solve_input.measure_policy,
        preferred_theta,
        progress_observer,
    )?;

    if solve_input.measure_policy == TerminalAffineMeasurePolicy::RiskNeutralMartingale
        && let Some(theta_warm_start_state) = theta_warm_start_state
    {
        theta_warm_start_state.record_theta_solution(
            solve_input.theta_warm_start_key,
            theta_solution.theta,
            theta_solution.buy_activity_premium_theta_derivative,
            theta_solution.sell_activity_premium_theta_derivative,
        );
    }

    Ok(TerminalAffineExpectedLogReturn {
        maturity_interval: solve_input.maturity_interval,
        expected_log_return: expected_log_return_from_theta_solution(&context, theta_solution)?,
        log_expected_price_ratio: log_expected_price_ratio_from_theta_solution(
            &context,
            theta_solution,
        )?,
    })
}

fn resolve_terminal_affine_spectral_domain(
    pricing_request: &VanillaOptionPricingRequest,
    mean_reversion_path: &TerminalAffineMeanReversionPath,
    spectral_domain_input: TerminalAffineSpectralDomainInput<'_>,
) -> Result<SpectralDomain, VanillaOptionPricingRunError> {
    match spectral_domain_input {
        TerminalAffineSpectralDomainInput::PayoffAnchored {
            strikes,
            payoff_types,
            collocation_node_count_override,
        } => build_spectral_domain_from_anchor_span(
            pricing_request,
            mean_reversion_path.terminal_anchor_value,
            mean_reversion_path.anchor_span,
            strikes,
            payoff_types,
            collocation_node_count_override,
        ),
        TerminalAffineSpectralDomainInput::ExplicitLogPrice(domain) => {
            SpectralDomain::try_new_explicit(
                domain.lower_bound - mean_reversion_path.terminal_anchor_value,
                domain.upper_bound - mean_reversion_path.terminal_anchor_value,
                domain.node_count,
            )
        }
    }
}

fn validate_terminal_affine_inputs(
    maturity_count: usize,
    interval_deltas: &[f64],
    interval_factor_grids: &[VanillaOptionPricingIntervalFactors],
) -> Result<(), VanillaOptionPricingRunError> {
    if maturity_count == 0
        || maturity_count > interval_factor_grids.len()
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

    let martingale_tolerance = terminal_affine_martingale_root_tolerance();
    let (theta, root_evaluation) =
        build_local_tilt_bracket_with_search_domain_preferred_theta_and_residual_evaluator(
            search_domain,
            target_log_moment_exponent,
            martingale_tolerance,
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
        target_log_moment_exponent,
        observed_log_moment_exponent: root_evaluation.observed_log_moment_exponent,
        martingale_residual: root_evaluation.residual,
        martingale_tolerance,
        buy_activity_premium_theta_derivative: -buy_residual_derivative / residual_theta_derivative,
        sell_activity_premium_theta_derivative: -sell_residual_derivative
            / residual_theta_derivative,
    })
}

fn solve_terminal_affine_measure_theta(
    context: &TerminalAffineMaturityContext<'_>,
    measure_policy: TerminalAffineMeasurePolicy,
    preferred_theta: Option<f64>,
    progress_observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<TerminalAffineThetaSolution, VanillaOptionPricingRunError> {
    match measure_policy {
        TerminalAffineMeasurePolicy::Physical => solve_terminal_affine_physical_theta(context),
        TerminalAffineMeasurePolicy::RiskNeutralMartingale => {
            solve_terminal_affine_martingale_theta(context, preferred_theta, progress_observer)
        }
    }
}

fn solve_terminal_affine_physical_theta(
    context: &TerminalAffineMaturityContext<'_>,
) -> Result<TerminalAffineThetaSolution, VanillaOptionPricingRunError> {
    let theta = 0.0;

    let mut cache = TerminalAffineCumulantCache::default();

    let denominator = cache
        .evaluate(context, TerminalAffineCumulantArgument::from_theta(theta))
        .map_err(|source| terminal_affine_measure_change_failure(context, source.to_string()))?;

    Ok(TerminalAffineThetaSolution {
        theta,
        denominator,
        target_log_moment_exponent: 0.0,
        observed_log_moment_exponent: 0.0,
        martingale_residual: 0.0,
        martingale_tolerance: terminal_affine_martingale_root_tolerance(),
        buy_activity_premium_theta_derivative: 0.0,
        sell_activity_premium_theta_derivative: 0.0,
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
        .map_err(|source| terminal_affine_residual_error(theta, source))?;

    let denominator = cache
        .evaluate_with_progress(
            context,
            TerminalAffineCumulantArgument::from_theta(theta),
            progress_observer,
        )
        .map_err(|source| terminal_affine_residual_error(theta, source))?;

    let log_moment = terminal_affine_first_log_moment(context, numerator, denominator);

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
        AnchoredLocalTiltSolveError::InvalidPricingRequest { source } => {
            format!("terminal affine pricing request is invalid: {source}")
        }
        AnchoredLocalTiltSolveError::InvalidReturnDecomposition { source } => {
            format!("terminal affine return decomposition is invalid: {source}")
        }
        AnchoredLocalTiltSolveError::BuySideEsscherTransformFailure { theta, source } => {
            format!("terminal affine buy-side transform failed at theta={theta}: {source}")
        }
        AnchoredLocalTiltSolveError::SellSideEsscherTransformFailure { theta, source } => {
            format!("terminal affine sell-side transform failed at theta={theta}: {source}")
        }
        AnchoredLocalTiltSolveError::NonFiniteTransformInput { theta, value } => {
            format!(
                "terminal affine martingale solve produced a non-finite transform input at theta={theta}: {value:?}"
            )
        }
        AnchoredLocalTiltSolveError::TransformEvaluationFailure { theta, message } => {
            format!("terminal affine cumulant could not be evaluated at theta={theta}: {message}")
        }
        AnchoredLocalTiltSolveError::NonRealObservedLogMomentExponent { theta, value } => {
            format!(
                "terminal affine martingale solve produced a non-real observed log moment exponent at theta={theta}: {value:?}"
            )
        }
        AnchoredLocalTiltSolveError::OutOfDomainStateRequest {
            theta,
            argument_imaginary,
            lower_theta_exclusive,
            upper_theta_exclusive,
        } => format!(
            "terminal affine martingale solve requested an out-of-domain state at theta={theta} for argument_imaginary={argument_imaginary} with admissible_theta_interval=({lower_theta_exclusive}, {upper_theta_exclusive})"
        ),
        AnchoredLocalTiltSolveError::InvalidTolerance { value } => {
            format!(
                "terminal affine martingale solve tolerance must be finite and strictly positive, found {value}"
            )
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
        AnchoredLocalTiltSolveError::UndefinedImplicitThetaSensitivity {
            theta,
            residual_theta_derivative,
        } => format!(
            "terminal affine martingale implicit sensitivity is undefined at theta={theta} because residual_theta_derivative={residual_theta_derivative}"
        ),
    }
}

fn terminal_affine_residual_error(
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
    spectral_domain: SpectralDomain,
    theta_solution: TerminalAffineThetaSolution,
    include_node_price_sensitivities: bool,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
) -> Result<Vec<TerminalAffineFrequencyOutput>, VanillaOptionPricingRunError> {
    if let Some(pricing_thread_pool) = pricing_thread_pool {
        return install_pricing_parallelism(Some(pricing_thread_pool), || {
            (0..spectral_domain.node_count)
                .into_par_iter()
                .map(|frequency_index| {
                    solve_terminal_affine_frequency_output(
                        context,
                        spectral_domain,
                        theta_solution,
                        include_node_price_sensitivities,
                        frequency_index,
                    )
                })
                .collect()
        });
    }

    (0..spectral_domain.node_count)
        .map(|frequency_index| {
            solve_terminal_affine_frequency_output(
                context,
                spectral_domain,
                theta_solution,
                include_node_price_sensitivities,
                frequency_index,
            )
        })
        .collect()
}

fn solve_terminal_affine_frequency_output(
    context: &TerminalAffineMaturityContext<'_>,
    spectral_domain: SpectralDomain,
    theta_solution: TerminalAffineThetaSolution,
    include_node_price_sensitivities: bool,
    frequency_index: usize,
) -> Result<TerminalAffineFrequencyOutput, VanillaOptionPricingRunError> {
    let frequency = spectral_domain.basis_frequency(frequency_index);

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

#[cfg(test)]
mod tests {
    use janiform_foundations::numeric::square_root_machine_epsilon;
    use janiform_model_kernels::long_variance::{LongVarianceParameters, LongVarianceState};

    use super::{
        TerminalAffineMaturitySolveInput, TerminalAffineMeasurePolicy,
        TerminalAffineSpectralDomainInput, solve_terminal_affine_maturity_law,
    };
    use crate::tests::support::build_valid_vanilla_option_request;
    use crate::{
        VanillaOptionPayoffType, VanillaOptionPricingIntervalFactors,
        VanillaOptionPricingModelParameters,
    };

    #[test]
    fn terminal_affine_measure_change_normalizes_characteristic_and_martingale() {
        let pricing_request = build_valid_vanilla_option_request();
        let solved_maturity = solve_test_maturity_law(pricing_request);

        let zero_frequency_characteristic = solved_maturity.characteristic_function_values[0];

        assert!((zero_frequency_characteristic.real - 1.0).abs() <= square_root_machine_epsilon());
        assert!(zero_frequency_characteristic.imaginary.abs() <= square_root_machine_epsilon());

        let measure_change = solved_maturity
            .terminal_affine_measure_change
            .expect("terminal affine measure-change diagnostics should be present");

        assert!(measure_change.absolute_martingale_residual <= measure_change.martingale_tolerance);
    }

    #[test]
    fn long_variance_state_changes_terminal_affine_characteristic_function() {
        let mut unit_state_request = build_valid_vanilla_option_request();

        unit_state_request.model_parameters =
            model_parameters_with_long_variance(active_long_variance_parameters());

        let mut elevated_state_request = unit_state_request.clone();

        elevated_state_request
            .boundary_input
            .long_variance_boundary_state =
            LongVarianceState::try_new(1.5).expect("elevated long-variance state should construct");

        let unit_state_solution = solve_test_maturity_law(unit_state_request);
        let elevated_state_solution = solve_test_maturity_law(elevated_state_request);

        assert_ne!(
            unit_state_solution.characteristic_function_values[1],
            elevated_state_solution.characteristic_function_values[1]
        );
    }

    fn solve_test_maturity_law(
        pricing_request: crate::VanillaOptionPricingRequest,
    ) -> super::SolvedMaturityLaw {
        let interval_factor_grids = [VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
            .expect("interval factors should construct")];

        let interval_deltas = [1.0_f64];
        let payoff_types = [VanillaOptionPayoffType::Call];

        let driver_parameters = pricing_request
            .model_parameters
            .nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .expect("NIG driver parameters should derive");

        let mut progress_observer = None;

        solve_terminal_affine_maturity_law(
            TerminalAffineMaturitySolveInput {
                pricing_request: &pricing_request,
                maturity_count: 1,
                maturity_interval: 1.0,
                spectral_domain_input: TerminalAffineSpectralDomainInput::PayoffAnchored {
                    strikes: pricing_request.market_inputs.strikes.as_slice(),
                    payoff_types: &payoff_types,
                    collocation_node_count_override: Some(257),
                },
                interval_deltas: &interval_deltas,
                interval_factor_grids: &interval_factor_grids,
                driver_parameters,
                measure_policy: TerminalAffineMeasurePolicy::RiskNeutralMartingale,
                include_node_price_sensitivities: false,
                pricing_thread_pool: None,
                theta_warm_start_key: super::SpectralThetaWarmStartKey::new(
                    0.02_f64.to_bits(),
                    0.01_f64.to_bits(),
                    1,
                ),
            },
            None,
            &mut progress_observer,
        )
        .expect("terminal affine maturity law should solve")
    }

    fn model_parameters_with_long_variance(
        long_variance_parameters: LongVarianceParameters,
    ) -> VanillaOptionPricingModelParameters {
        let base_parameters = build_valid_vanilla_option_request().model_parameters;

        VanillaOptionPricingModelParameters::try_new(
            base_parameters.hawkes_kernel_parameters,
            base_parameters.news_process_configuration,
            base_parameters.nig_shared_plus_shift_configuration,
            base_parameters.return_clock_exposure_parameters,
            long_variance_parameters,
            base_parameters.pricing_kernel_parameters,
        )
        .expect("model parameters should construct")
    }

    fn active_long_variance_parameters() -> LongVarianceParameters {
        LongVarianceParameters::try_new(5.0, 0.35, 0.03, 0.04)
            .expect("active long-variance parameters should construct")
    }
}
