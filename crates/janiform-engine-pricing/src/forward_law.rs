use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};

use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult;
use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};
use janiform_model_kernels::nig::NigBuySellDriverParameters;

use super::constants::INTERNAL_FORWARD_LAW_FREQUENCY_GRID_POLICY;
use super::contracts::{
    AnchoredPricingRecursionDiagnostics, AnchoredPricingRecursionIntervalDiagnostics,
    VanillaOptionPayoffType, VanillaOptionPricingMarketInputs, VanillaOptionPricingRequest,
};
use super::implied_volatility::{
    VanillaOptionImpliedVolatilityInversionOutcome, invert_vanilla_option_implied_volatility,
};
use super::integration_geometry::{
    estimate_asymptotic_tail_integral_error, initial_midpoint_upper_bound_from_phase_scale,
    next_power_of_two_at_least,
};
use super::interval_schedule::PricingIntervalSchedule;
use super::probability_projection::probability_mass_from_sampled_cdf_values;
use super::progress::PricingProgressEvent;
use super::progress_runtime::PricingProgressReporter;
use super::runner::{
    DeviationGrid, LocalTransitionRequestContext, LocalTransitionRowState,
    PricingCharacteristicFunctionEvaluation, RiskNeutralAnchorMaterialization,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingRunError, build_deviation_grid,
    build_pricing_risk_neutral_anchor_materialization,
    derive_local_tilted_interval_expectations_for_row_with_parallelism,
    evaluate_core_log_return_characteristic_function_under_local_measure_with_parallelism,
    resolve_local_transition_theta_for_row_with_parallelism,
};
use super::thread_pool::{install_pricing_parallelism, resolved_pricing_cpu_count};
use super::time_scaling::intraday_year_fraction;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ForwardLawTerminalSnapshot {
    pub(crate) maturity_interval: f64,
    pub(crate) terminal_anchor_value: f64,
    pub(crate) deviation_grid: DeviationGrid,
    pub(crate) terminal_deviation_nodes: Vec<f64>,
    pub(crate) terminal_probability_mass: Vec<f64>,
    pub(crate) anchored_recursion_diagnostics: AnchoredPricingRecursionDiagnostics,
    pub(crate) representative_transform_diagnostics:
        Option<ExponentialAffineTransformIntegrationResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ForwardLawScopeSolution {
    pub(crate) terminal_snapshots: Vec<ForwardLawTerminalSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ForwardPricedStrike {
    pub(crate) price: f64,
    pub(crate) implied_volatility_outcome: VanillaOptionImpliedVolatilityInversionOutcome,
    pub(crate) forward_price: f64,
    pub(crate) discount_factor: f64,
    pub(crate) boundary_state_norm: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct TerminalDistributionPricingKernel {
    terminal_spots: Vec<f64>,
    prefix_probability_mass: Vec<f64>,
    prefix_weighted_spot_mass: Vec<f64>,
    total_weighted_spot_mass: f64,
}

impl TerminalDistributionPricingKernel {
    fn from_terminal_snapshot(
        pricing_request: &VanillaOptionPricingRequest,
        terminal_snapshot: &ForwardLawTerminalSnapshot,
    ) -> Self {
        let current_observed_log_price = pricing_request
            .boundary_input
            .observed_boundary_state
            .observed_log_price;

        let terminal_spots = terminal_snapshot
            .terminal_deviation_nodes
            .iter()
            .map(|deviation| {
                pricing_request.market_inputs.spot
                    * (terminal_snapshot.terminal_anchor_value + *deviation
                        - current_observed_log_price)
                        .exp()
            })
            .collect::<Vec<_>>();

        let mut prefix_probability_mass =
            Vec::with_capacity(terminal_snapshot.terminal_probability_mass.len());
        let mut prefix_weighted_spot_mass =
            Vec::with_capacity(terminal_snapshot.terminal_probability_mass.len());
        let mut running_probability_mass = 0.0;
        let mut running_weighted_spot_mass = 0.0;

        for (probability_mass, terminal_spot) in terminal_snapshot
            .terminal_probability_mass
            .iter()
            .copied()
            .zip(terminal_spots.iter().copied())
        {
            running_probability_mass += probability_mass;
            running_weighted_spot_mass += probability_mass * terminal_spot;
            prefix_probability_mass.push(running_probability_mass);
            prefix_weighted_spot_mass.push(running_weighted_spot_mass);
        }

        Self {
            terminal_spots,
            prefix_probability_mass,
            prefix_weighted_spot_mass,
            total_weighted_spot_mass: running_weighted_spot_mass,
        }
    }

    fn discounted_price(
        &self,
        payoff_type: VanillaOptionPayoffType,
        strike: f64,
        discount_factor: f64,
    ) -> f64 {
        // Terminal spots inherit the ordered deviation grid, so one partition
        // point is enough to price all in-the-money mass exactly.
        let first_in_the_money_index =
            self.terminal_spots
                .partition_point(|spot| match payoff_type {
                    VanillaOptionPayoffType::Call => *spot <= strike,
                    VanillaOptionPayoffType::Put => *spot < strike,
                });

        let undiscounted_price = match payoff_type {
            VanillaOptionPayoffType::Call => {
                let tail_probability_mass = if first_in_the_money_index == 0 {
                    self.prefix_probability_mass.last().copied().unwrap_or(0.0)
                } else if first_in_the_money_index == self.terminal_spots.len() {
                    0.0
                } else {
                    self.prefix_probability_mass.last().copied().unwrap_or(0.0)
                        - self.prefix_probability_mass[first_in_the_money_index - 1]
                };
                let tail_weighted_spot_mass = if first_in_the_money_index == 0 {
                    self.total_weighted_spot_mass
                } else if first_in_the_money_index == self.terminal_spots.len() {
                    0.0
                } else {
                    self.total_weighted_spot_mass
                        - self.prefix_weighted_spot_mass[first_in_the_money_index - 1]
                };

                tail_weighted_spot_mass - strike * tail_probability_mass
            }
            VanillaOptionPayoffType::Put => {
                if first_in_the_money_index == 0 {
                    0.0
                } else {
                    strike * self.prefix_probability_mass[first_in_the_money_index - 1]
                        - self.prefix_weighted_spot_mass[first_in_the_money_index - 1]
                }
            }
        };

        discount_factor * undiscounted_price
    }
}

pub(crate) fn solve_forward_law_scope(
    pricing_request: &VanillaOptionPricingRequest,
    runtime_interval_schedule: &PricingIntervalSchedule,
    strikes: &[f64],
    maturity_interval_counts: &[usize],
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    driver_parameters: NigBuySellDriverParameters,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    progress_reporter: &PricingProgressReporter,
) -> Result<ForwardLawScopeSolution, VanillaOptionPricingRunError> {
    let risk_neutral_anchor_materialization = build_pricing_risk_neutral_anchor_materialization(
        pricing_request,
        runtime_interval_schedule,
        interval_factors,
        driver_parameters,
        &mut None,
    )?;

    let mut unique_maturity_interval_counts = maturity_interval_counts.to_vec();

    unique_maturity_interval_counts.sort_unstable();
    unique_maturity_interval_counts.dedup();

    let coarse_grid = build_common_deviation_grid(
        pricing_request,
        runtime_interval_schedule,
        strikes,
        &risk_neutral_anchor_materialization,
        unique_maturity_interval_counts.as_slice(),
        false,
        pricing_thread_pool,
        cpu_count,
    )?;
    let refined_grid = build_common_deviation_grid(
        pricing_request,
        runtime_interval_schedule,
        strikes,
        &risk_neutral_anchor_materialization,
        unique_maturity_interval_counts.as_slice(),
        true,
        pricing_thread_pool,
        cpu_count,
    )?;

    let solved_snapshots = propagate_terminal_probability_mass(
        pricing_request,
        runtime_interval_schedule,
        interval_factors,
        driver_parameters,
        &risk_neutral_anchor_materialization,
        coarse_grid,
        refined_grid,
        unique_maturity_interval_counts.as_slice(),
        pricing_thread_pool,
        cpu_count,
        progress_reporter,
    )?;

    let terminal_snapshots = maturity_interval_counts
        .iter()
        .map(|requested_count| {
            solved_snapshots
                .iter()
                .find(|snapshot| {
                    snapshot.maturity_interval
                        == runtime_interval_schedule
                            .maturity_interval_hours(*requested_count - 1)
                            .unwrap_or(f64::NAN)
                })
                .cloned()
                .ok_or(VanillaOptionPricingRunError::InvalidObservedBoundaryStateDuringRecursion {
                    interval_index: pricing_request.boundary_input.interval_index,
                    deviation: pricing_request.boundary_input.observed_boundary_state.deviation,
                    message: format!(
                        "missing propagated terminal snapshot for maturity interval count {requested_count}"
                    ),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ForwardLawScopeSolution { terminal_snapshots })
}

pub(crate) fn price_terminal_snapshot_strikes(
    pricing_request: &VanillaOptionPricingRequest,
    terminal_snapshot: &ForwardLawTerminalSnapshot,
    strikes: &[f64],
    maturity_index: usize,
    total_maturities: usize,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    progress_reporter: &PricingProgressReporter,
) -> Vec<ForwardPricedStrike> {
    let payoff_type = pricing_request.payoff.payoff_type;
    let maturity_interval = terminal_snapshot.maturity_interval;

    let maturity_year_fraction = intraday_year_fraction(maturity_interval);

    let risk_free_rate = pricing_request.market_inputs.risk_free_rate;
    let dividend_yield = pricing_request.market_inputs.dividend_yield;

    let boundary_state = pricing_request.boundary_input.hawkes_boundary_state;
    let boundary_state_norm = (boundary_state.y_plus_plus * boundary_state.y_plus_plus
        + boundary_state.y_plus_minus * boundary_state.y_plus_minus
        + boundary_state.y_minus_plus * boundary_state.y_minus_plus
        + boundary_state.y_minus_minus * boundary_state.y_minus_minus)
        .sqrt();

    let forward_price = pricing_request.market_inputs.spot
        * ((risk_free_rate - dividend_yield) * maturity_year_fraction).exp();
    let discount_factor = (-risk_free_rate * maturity_year_fraction).exp();
    let terminal_distribution = TerminalDistributionPricingKernel::from_terminal_snapshot(
        pricing_request,
        terminal_snapshot,
    );

    let price_strike = |strike: f64| {
        let price = terminal_distribution.discounted_price(payoff_type, strike, discount_factor);

        let implied_volatility_outcome = invert_vanilla_option_implied_volatility(
            &VanillaOptionPricingMarketInputs::new_single_node(
                pricing_request.market_inputs.spot,
                strike,
                maturity_interval,
                risk_free_rate,
                dividend_yield,
            ),
            payoff_type,
            price,
        );

        ForwardPricedStrike {
            price,
            implied_volatility_outcome,
            forward_price,
            discount_factor,
            boundary_state_norm,
        }
    };

    progress_reporter.start_strike_pricing(maturity_index, total_maturities, strikes.len());

    let priced_strikes = if resolved_pricing_cpu_count(cpu_count) <= 1 {
        strikes
            .iter()
            .copied()
            .map(|strike| {
                let priced_strike = price_strike(strike);
                progress_reporter.advance_strike_pricing(1);
                priced_strike
            })
            .collect::<Vec<_>>()
    } else {
        install_pricing_parallelism(pricing_thread_pool, || {
            strikes
                .par_iter()
                .copied()
                .map(|strike| {
                    let priced_strike = price_strike(strike);
                    progress_reporter.advance_strike_pricing(1);
                    priced_strike
                })
                .collect::<Vec<_>>()
        })
    };

    progress_reporter.finish_strike_pricing();

    priced_strikes
}

fn build_common_deviation_grid(
    pricing_request: &VanillaOptionPricingRequest,
    runtime_interval_schedule: &PricingIntervalSchedule,
    strikes: &[f64],
    risk_neutral_anchor_materialization: &RiskNeutralAnchorMaterialization,
    maturity_interval_counts: &[usize],
    refined: bool,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<DeviationGrid, VanillaOptionPricingRunError> {
    #[derive(Clone, Copy)]
    struct DeviationGridBoundsSummary {
        lower_bound: f64,
        upper_bound: f64,
        step: f64,
    }

    let summarize_maturity = |maturity_interval_count: usize| {
        let mut maturity_lower_bound = f64::INFINITY;
        let mut maturity_upper_bound = f64::NEG_INFINITY;
        let mut maturity_step = f64::INFINITY;

        for strike in strikes {
            let strike_request = pricing_request_for_strike(
                pricing_request,
                runtime_interval_schedule,
                *strike,
                maturity_interval_count,
            )?;

            let strike_grid = build_deviation_grid(
                &strike_request,
                risk_neutral_anchor_materialization,
                maturity_interval_count,
                refined,
            )?;

            let strike_step = if strike_grid.node_count <= 1 {
                0.0
            } else {
                (strike_grid.upper_bound - strike_grid.lower_bound)
                    / (strike_grid.node_count - 1) as f64
            };

            maturity_lower_bound = maturity_lower_bound.min(strike_grid.lower_bound);
            maturity_upper_bound = maturity_upper_bound.max(strike_grid.upper_bound);
            maturity_step = maturity_step.min(strike_step);
        }

        Ok::<_, VanillaOptionPricingRunError>(DeviationGridBoundsSummary {
            lower_bound: maturity_lower_bound,
            upper_bound: maturity_upper_bound,
            step: maturity_step,
        })
    };

    let summaries =
        if resolved_pricing_cpu_count(cpu_count) <= 1 || maturity_interval_counts.len() <= 1 {
            maturity_interval_counts
                .iter()
                .copied()
                .map(summarize_maturity)
                .collect::<Result<Vec<_>, _>>()?
        } else {
            install_pricing_parallelism(pricing_thread_pool, || {
                maturity_interval_counts
                    .par_iter()
                    .copied()
                    .map(summarize_maturity)
                    .collect::<Result<Vec<_>, _>>()
            })?
        };

    let mut common_lower_bound = f64::INFINITY;
    let mut common_upper_bound = f64::NEG_INFINITY;
    let mut common_step = f64::INFINITY;

    for summary in summaries {
        common_lower_bound = common_lower_bound.min(summary.lower_bound);
        common_upper_bound = common_upper_bound.max(summary.upper_bound);
        common_step = common_step.min(summary.step);
    }

    if !common_lower_bound.is_finite()
        || !common_upper_bound.is_finite()
        || common_lower_bound >= common_upper_bound
        || !common_step.is_finite()
        || common_step <= 0.0
    {
        return Err(
            VanillaOptionPricingRunError::InvalidObservedBoundaryStateDuringRecursion {
                interval_index: pricing_request.boundary_input.interval_index,
                deviation: pricing_request
                    .boundary_input
                    .observed_boundary_state
                    .deviation,
                message: "failed to derive a finite common deviation grid".to_string(),
            },
        );
    }

    let interval_count_float = ((common_upper_bound - common_lower_bound) / common_step).ceil();
    let mut interval_count = interval_count_float as usize;
    if interval_count % 2 != 0 {
        interval_count += 1;
    }

    Ok(DeviationGrid {
        lower_bound: common_lower_bound,
        upper_bound: common_upper_bound,
        node_count: interval_count + 1,
    })
}

fn propagate_terminal_probability_mass(
    pricing_request: &VanillaOptionPricingRequest,
    runtime_interval_schedule: &PricingIntervalSchedule,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    driver_parameters: NigBuySellDriverParameters,
    risk_neutral_anchor_materialization: &RiskNeutralAnchorMaterialization,
    coarse_grid: DeviationGrid,
    refined_grid: DeviationGrid,
    requested_maturity_interval_counts: &[usize],
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    progress_reporter: &PricingProgressReporter,
) -> Result<Vec<ForwardLawTerminalSnapshot>, VanillaOptionPricingRunError> {
    let initial_probability_mass = interpolate_initial_probability_mass(
        refined_grid,
        pricing_request
            .boundary_input
            .observed_boundary_state
            .deviation,
    );

    let mut current_probability_mass = initial_probability_mass;
    let mut interval_diagnostics = Vec::with_capacity(interval_factors.len());
    let mut representative_transform_diagnostics = None;
    let mut terminal_snapshots = Vec::with_capacity(requested_maturity_interval_counts.len());

    for (interval_index, interval_factor) in interval_factors.iter().copied().enumerate() {
        progress_reporter.emit(PricingProgressEvent::interval_progress(
            interval_index,
            interval_factors.len(),
            Some(interval_index),
            interval_factors.len(),
        ));

        let applied_transition = apply_transition_matrix(
            pricing_request,
            interval_factor,
            driver_parameters,
            risk_neutral_anchor_materialization,
            refined_grid,
            interval_index,
            current_probability_mass.as_slice(),
            pricing_thread_pool,
            cpu_count,
            progress_reporter,
        )?;

        current_probability_mass = applied_transition.next_probability_mass;

        interval_diagnostics.push(applied_transition.interval_diagnostics);

        if representative_transform_diagnostics.is_none() {
            representative_transform_diagnostics =
                applied_transition.representative_transform_diagnostics;
        }

        let current_interval_count = interval_index + 1;
        if requested_maturity_interval_counts.contains(&current_interval_count) {
            let anchored_recursion_diagnostics = AnchoredPricingRecursionDiagnostics::try_new(
                coarse_grid.lower_bound,
                coarse_grid.upper_bound,
                coarse_grid.node_count,
                refined_grid.node_count,
                0.0,
                0.0,
                interval_diagnostics.clone(),
            )
            .map_err(|source| {
                VanillaOptionPricingRunError::InvalidAnchoredRecursionDiagnostics { source }
            })?;

            terminal_snapshots.push(ForwardLawTerminalSnapshot {
                maturity_interval: runtime_interval_schedule
                    .maturity_interval_hours(current_interval_count - 1)
                    .map_err(|source| {
                        VanillaOptionPricingRunError::InvalidObservedBoundaryStateDuringRecursion {
                            interval_index: pricing_request.boundary_input.interval_index,
                            deviation: pricing_request
                                .boundary_input
                                .observed_boundary_state
                                .deviation,
                            message: source.to_string(),
                        }
                    })?,
                terminal_anchor_value: risk_neutral_anchor_materialization
                    .runtime_interval_anchors
                    .end_anchor_values[current_interval_count - 1],
                deviation_grid: refined_grid,
                terminal_deviation_nodes: refined_grid.nodes(),
                terminal_probability_mass: current_probability_mass.clone(),
                anchored_recursion_diagnostics,
                representative_transform_diagnostics,
            });
        }
    }

    progress_reporter.emit(PricingProgressEvent::interval_progress(
        interval_factors.len(),
        interval_factors.len(),
        None,
        interval_factors.len(),
    ));

    Ok(terminal_snapshots)
}

fn interpolate_initial_probability_mass(
    deviation_grid: DeviationGrid,
    observed_deviation: f64,
) -> Vec<f64> {
    if deviation_grid.node_count == 1 {
        return vec![1.0];
    }

    if observed_deviation <= deviation_grid.lower_bound {
        let mut probability_mass = vec![0.0; deviation_grid.node_count];
        probability_mass[0] = 1.0;
        return probability_mass;
    }

    if observed_deviation >= deviation_grid.upper_bound {
        let mut probability_mass = vec![0.0; deviation_grid.node_count];
        probability_mass[deviation_grid.node_count - 1] = 1.0;
        return probability_mass;
    }

    let step = (deviation_grid.upper_bound - deviation_grid.lower_bound)
        / (deviation_grid.node_count - 1) as f64;
    let lower_index = ((observed_deviation - deviation_grid.lower_bound) / step).floor() as usize;
    let lower_node = deviation_grid.lower_bound + step * lower_index as f64;
    let interpolation_weight = (observed_deviation - lower_node) / step;

    let mut probability_mass = vec![0.0; deviation_grid.node_count];

    probability_mass[lower_index] = 1.0 - interpolation_weight;
    probability_mass[lower_index + 1] = interpolation_weight;

    probability_mass
}

#[derive(Debug, Clone, PartialEq)]
struct AppliedIntervalTransition {
    next_probability_mass: Vec<f64>,
    interval_diagnostics: AnchoredPricingRecursionIntervalDiagnostics,
    representative_transform_diagnostics: Option<ExponentialAffineTransformIntegrationResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionRowExecutionMode {
    Sequential,
    AcrossRows,
    WithinEachRow,
}

fn resolve_transition_row_execution_mode(
    active_row_count: usize,
    cpu_count: Option<usize>,
) -> TransitionRowExecutionMode {
    let worker_count = resolved_pricing_cpu_count(cpu_count);
    if worker_count <= 1 {
        return TransitionRowExecutionMode::Sequential;
    }

    if active_row_count >= worker_count {
        TransitionRowExecutionMode::AcrossRows
    } else {
        TransitionRowExecutionMode::WithinEachRow
    }
}

fn interval_supports_shift_invariant_transition(
    pricing_request: &VanillaOptionPricingRequest,
    interval_delta: f64,
) -> Result<bool, VanillaOptionPricingRunError> {
    let deviation_decay_factor = (-pricing_request
        .boundary_input
        .mean_reversion_fitted_parameters
        .kappa
        * interval_delta)
        .exp();

    Ok(deviation_decay_factor == 1.0)
}

#[allow(clippy::too_many_arguments)]
fn apply_shift_invariant_transition_rows(
    transition_context: &LocalTransitionRequestContext,
    driver_parameters: NigBuySellDriverParameters,
    nodes: &[f64],
    boundaries: &[f64],
    active_rows: &[(usize, f64, f64)],
    row_execution_mode: TransitionRowExecutionMode,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    progress_reporter: &PricingProgressReporter,
) -> Result<IntervalTransitionAccumulation, VanillaOptionPricingRunError> {
    if nodes.is_empty() {
        return Ok(IntervalTransitionAccumulation::new(0));
    }
    if active_rows.is_empty() {
        return Ok(IntervalTransitionAccumulation::new(nodes.len()));
    }
    if boundaries.is_empty() {
        let mut accumulation = IntervalTransitionAccumulation::new(nodes.len());
        for (_, _, current_mass) in active_rows.iter().copied() {
            accumulation.record_row(AppliedTransitionRow {
                weighted_probability_mass: vec![current_mass],
                maximum_tail_probability_mass: 1.0,
                row_probability_mass_error: 0.0,
                solved_theta: 0.0,
                representative_transform_diagnostics: None,
            });
            progress_reporter.advance_transition_rows(1);
        }
        return Ok(accumulation);
    }

    let reference_row = transition_context.build_row_state(nodes[0])?;
    let enable_frequency_parallelism = matches!(
        row_execution_mode,
        TransitionRowExecutionMode::WithinEachRow
    );
    let row_pricing_thread_pool = enable_frequency_parallelism
        .then_some(pricing_thread_pool)
        .flatten();
    let row_cpu_count = enable_frequency_parallelism.then_some(cpu_count).flatten();

    let (reference_theta, _) = resolve_local_transition_theta_for_row_with_parallelism(
        transition_context,
        &reference_row,
        driver_parameters,
        row_pricing_thread_pool,
        row_cpu_count,
    )?;
    let reference_expectations =
        derive_local_tilted_interval_expectations_for_row_with_parallelism(
            transition_context,
            &reference_row,
            driver_parameters,
            reference_theta,
            row_pricing_thread_pool,
            row_cpu_count,
        )?;

    let shift = nodes[0] - reference_expectations.core_expected_increment;
    let first_boundary = boundaries.first().copied().unwrap_or(0.0) - shift;
    let boundary_step = if boundaries.len() <= 1 {
        0.0
    } else {
        boundaries[1] - boundaries[0]
    };
    let boundary_extension = boundaries.len();
    let extended_first_boundary = first_boundary - boundary_extension as f64 * boundary_step;

    let cdf_evaluator = SharedRiskNeutralCoreCdfEvaluator::new(
        transition_context,
        reference_row.boundary_state.deviation,
        driver_parameters,
        reference_theta,
    )?;

    let (extended_cdf_values, representative_transform_diagnostics) = cdf_evaluator
        .evaluate_uniform_boundaries(
            extended_first_boundary,
            boundary_step,
            boundaries.len().saturating_mul(2),
            enable_frequency_parallelism,
            pricing_thread_pool,
            cpu_count,
        )?;

    let accumulate_active_row = |source_node_index: usize, current_mass: f64| {
        let sampled_cdf_start = boundary_extension.saturating_sub(source_node_index);
        let sampled_cdf_end = sampled_cdf_start + boundaries.len();
        let row_probability_mass = probability_mass_from_sampled_cdf_values(
            &extended_cdf_values[sampled_cdf_start..sampled_cdf_end],
        );
        let row_probability_mass_error = (row_probability_mass.iter().sum::<f64>() - 1.0).abs();
        let maximum_tail_probability_mass =
            row_probability_mass[0].max(row_probability_mass[row_probability_mass.len() - 1]);
        let weighted_probability_mass = row_probability_mass
            .into_iter()
            .map(|probability_mass| probability_mass * current_mass)
            .collect::<Vec<_>>();

        Ok::<_, VanillaOptionPricingRunError>(AppliedTransitionRow {
            weighted_probability_mass,
            maximum_tail_probability_mass,
            row_probability_mass_error,
            solved_theta: reference_theta,
            representative_transform_diagnostics: representative_transform_diagnostics.clone(),
        })
    };

    match row_execution_mode {
        TransitionRowExecutionMode::AcrossRows => {
            install_pricing_parallelism(pricing_thread_pool, || {
                active_rows
                    .par_iter()
                    .copied()
                    .try_fold(
                        || IntervalTransitionAccumulation::new(nodes.len()),
                        |mut accumulation, (source_node_index, _, current_mass)| {
                            let row_solution =
                                accumulate_active_row(source_node_index, current_mass)?;
                            accumulation.record_row(row_solution);
                            progress_reporter.advance_transition_rows(1);
                            Ok::<_, VanillaOptionPricingRunError>(accumulation)
                        },
                    )
                    .try_reduce(
                        || IntervalTransitionAccumulation::new(nodes.len()),
                        |left, right| Ok::<_, VanillaOptionPricingRunError>(left.merge(right)),
                    )
            })
        }
        TransitionRowExecutionMode::Sequential | TransitionRowExecutionMode::WithinEachRow => {
            let mut accumulation = IntervalTransitionAccumulation::new(nodes.len());
            for (source_node_index, _, current_mass) in active_rows.iter().copied() {
                let row_solution = accumulate_active_row(source_node_index, current_mass)?;
                accumulation.record_row(row_solution);
                progress_reporter.advance_transition_rows(1);
            }
            Ok(accumulation)
        }
    }
}

fn apply_transition_matrix(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factor: VanillaOptionPricingIntervalFactors,
    driver_parameters: NigBuySellDriverParameters,
    risk_neutral_anchor_materialization: &RiskNeutralAnchorMaterialization,
    deviation_grid: DeviationGrid,
    interval_index: usize,
    current_probability_mass: &[f64],
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    progress_reporter: &PricingProgressReporter,
) -> Result<AppliedIntervalTransition, VanillaOptionPricingRunError> {
    let nodes = deviation_grid.nodes();
    let boundaries = deviation_grid.boundaries();
    let global_interval_index = pricing_request.boundary_input.interval_index + interval_index;
    let runtime_anchors = &risk_neutral_anchor_materialization.runtime_interval_anchors;

    let current_anchor_value = if interval_index == 0 {
        runtime_anchors.start_anchor_value
    } else {
        runtime_anchors.end_anchor_values[interval_index - 1]
    };

    let current_pressure_state = if interval_index == 0 {
        runtime_anchors.start_pressure_state
    } else {
        runtime_anchors.end_pressure_states[interval_index - 1]
    };

    let next_anchor_value = runtime_anchors.end_anchor_values[interval_index];
    let interval_delta = runtime_anchors.interval_deltas[interval_index];

    let expected_hawkes_state_before =
        risk_neutral_anchor_materialization.expected_hawkes_state_path[interval_index];
    let expected_hawkes_state_after =
        risk_neutral_anchor_materialization.expected_hawkes_state_path[interval_index + 1];
    let transition_context = LocalTransitionRequestContext::from_interval_state(
        pricing_request,
        interval_factor,
        global_interval_index,
        expected_hawkes_state_before,
        current_anchor_value,
        current_pressure_state,
        interval_delta,
    )?;

    let active_rows = nodes
        .iter()
        .copied()
        .enumerate()
        .zip(current_probability_mass.iter().copied())
        .filter_map(|((node_index, deviation), current_mass)| {
            (current_mass != 0.0).then_some((node_index, deviation, current_mass))
        })
        .collect::<Vec<_>>();

    let row_execution_mode = resolve_transition_row_execution_mode(active_rows.len(), cpu_count);

    let solve_row = |deviation: f64, current_mass: f64, enable_frequency_parallelism: bool| {
        let transition_row = transition_context.build_row_state(deviation)?;

        let (local_transition_theta, _) = resolve_local_transition_theta_for_row_with_parallelism(
            &transition_context,
            &transition_row,
            driver_parameters,
            enable_frequency_parallelism
                .then_some(pricing_thread_pool)
                .flatten(),
            enable_frequency_parallelism.then_some(cpu_count).flatten(),
        )?;

        let local_expectations =
            derive_local_tilted_interval_expectations_for_row_with_parallelism(
                &transition_context,
                &transition_row,
                driver_parameters,
                local_transition_theta,
                enable_frequency_parallelism
                    .then_some(pricing_thread_pool)
                    .flatten(),
                enable_frequency_parallelism.then_some(cpu_count).flatten(),
            )?;

        let row = transition_probability_row_from_shared_frequency_grid(
            &transition_context,
            &transition_row,
            driver_parameters,
            local_transition_theta,
            local_expectations.core_expected_increment,
            &boundaries,
            enable_frequency_parallelism,
            pricing_thread_pool,
            cpu_count,
        )?;

        let row_mass = row.probability_mass.iter().sum::<f64>();
        let maximum_tail_probability_mass =
            row.probability_mass[0].max(row.probability_mass[row.probability_mass.len() - 1]);
        let mut weighted_probability_mass = row.probability_mass;
        for probability_mass in &mut weighted_probability_mass {
            *probability_mass *= current_mass;
        }

        Ok::<_, VanillaOptionPricingRunError>(AppliedTransitionRow {
            weighted_probability_mass,
            maximum_tail_probability_mass,
            row_probability_mass_error: (row_mass - 1.0).abs(),
            solved_theta: local_transition_theta,
            representative_transform_diagnostics: row.representative_transform_diagnostics,
        })
    };

    progress_reporter.start_transition_rows(
        interval_index,
        runtime_anchors.interval_deltas.len(),
        active_rows.len(),
    );
    let applied_rows =
        if interval_supports_shift_invariant_transition(pricing_request, interval_delta)? {
            apply_shift_invariant_transition_rows(
                &transition_context,
                driver_parameters,
                &nodes,
                &boundaries,
                active_rows.as_slice(),
                row_execution_mode,
                pricing_thread_pool,
                cpu_count,
                progress_reporter,
            )?
        } else {
            match row_execution_mode {
                TransitionRowExecutionMode::Sequential => {
                    let mut accumulation = IntervalTransitionAccumulation::new(nodes.len());
                    for (_, deviation, current_mass) in active_rows {
                        let row_solution = solve_row(deviation, current_mass, false)?;
                        accumulation.record_row(row_solution);
                        progress_reporter.advance_transition_rows(1);
                    }
                    accumulation
                }
                TransitionRowExecutionMode::AcrossRows => {
                    install_pricing_parallelism(pricing_thread_pool, || {
                        active_rows
                            .par_iter()
                            .copied()
                            .try_fold(
                                || IntervalTransitionAccumulation::new(nodes.len()),
                                |mut accumulation, (_, deviation, current_mass)| {
                                    let row_solution = solve_row(deviation, current_mass, false)?;
                                    accumulation.record_row(row_solution);
                                    progress_reporter.advance_transition_rows(1);
                                    Ok::<_, VanillaOptionPricingRunError>(accumulation)
                                },
                            )
                            .try_reduce(
                                || IntervalTransitionAccumulation::new(nodes.len()),
                                |left, right| {
                                    Ok::<_, VanillaOptionPricingRunError>(left.merge(right))
                                },
                            )
                    })?
                }
                TransitionRowExecutionMode::WithinEachRow => {
                    let mut accumulation = IntervalTransitionAccumulation::new(nodes.len());
                    for (_, deviation, current_mass) in active_rows {
                        let row_solution = solve_row(deviation, current_mass, true)?;
                        accumulation.record_row(row_solution);
                        progress_reporter.advance_transition_rows(1);
                    }
                    accumulation
                }
            }
        };

    progress_reporter.finish_transition_rows();

    let interval_diagnostics = AnchoredPricingRecursionIntervalDiagnostics::try_new(
        global_interval_index,
        current_anchor_value,
        next_anchor_value,
        interval_delta,
        expected_hawkes_state_before,
        expected_hawkes_state_after,
        applied_rows.maximum_tail_probability_mass,
        applied_rows.maximum_row_probability_mass_error,
        applied_rows.minimum_solved_theta(),
        applied_rows.maximum_solved_theta(),
        Vec::new(),
    )
    .map_err(
        |source| VanillaOptionPricingRunError::InvalidAnchoredRecursionDiagnostics { source },
    )?;

    Ok(AppliedIntervalTransition {
        next_probability_mass: applied_rows.next_probability_mass,
        interval_diagnostics,
        representative_transform_diagnostics: applied_rows.representative_transform_diagnostics,
    })
}

#[derive(Debug, Clone, PartialEq)]
struct AppliedTransitionRow {
    weighted_probability_mass: Vec<f64>,
    maximum_tail_probability_mass: f64,
    row_probability_mass_error: f64,
    solved_theta: f64,
    representative_transform_diagnostics: Option<ExponentialAffineTransformIntegrationResult>,
}

#[derive(Debug, Clone, PartialEq)]
struct IntervalTransitionAccumulation {
    next_probability_mass: Vec<f64>,
    maximum_tail_probability_mass: f64,
    maximum_row_probability_mass_error: f64,
    minimum_solved_theta: Option<f64>,
    maximum_solved_theta: Option<f64>,
    representative_transform_diagnostics: Option<ExponentialAffineTransformIntegrationResult>,
}

impl IntervalTransitionAccumulation {
    fn new(node_count: usize) -> Self {
        Self {
            next_probability_mass: vec![0.0; node_count],
            maximum_tail_probability_mass: 0.0,
            maximum_row_probability_mass_error: 0.0,
            minimum_solved_theta: None,
            maximum_solved_theta: None,
            representative_transform_diagnostics: None,
        }
    }

    fn record_row(&mut self, row: AppliedTransitionRow) {
        for (next_probability_mass, weighted_probability_mass) in self
            .next_probability_mass
            .iter_mut()
            .zip(row.weighted_probability_mass)
        {
            *next_probability_mass += weighted_probability_mass;
        }

        self.maximum_tail_probability_mass = self
            .maximum_tail_probability_mass
            .max(row.maximum_tail_probability_mass);
        self.maximum_row_probability_mass_error = self
            .maximum_row_probability_mass_error
            .max(row.row_probability_mass_error);
        self.minimum_solved_theta = Some(match self.minimum_solved_theta {
            Some(current_value) => current_value.min(row.solved_theta),
            None => row.solved_theta,
        });
        self.maximum_solved_theta = Some(match self.maximum_solved_theta {
            Some(current_value) => current_value.max(row.solved_theta),
            None => row.solved_theta,
        });

        if self.representative_transform_diagnostics.is_none() {
            self.representative_transform_diagnostics = row.representative_transform_diagnostics;
        }
    }

    fn merge(mut self, other: Self) -> Self {
        for (left_value, right_value) in self
            .next_probability_mass
            .iter_mut()
            .zip(other.next_probability_mass)
        {
            *left_value += right_value;
        }

        self.maximum_tail_probability_mass = self
            .maximum_tail_probability_mass
            .max(other.maximum_tail_probability_mass);
        self.maximum_row_probability_mass_error = self
            .maximum_row_probability_mass_error
            .max(other.maximum_row_probability_mass_error);
        self.minimum_solved_theta = match (self.minimum_solved_theta, other.minimum_solved_theta) {
            (Some(left_value), Some(right_value)) => Some(left_value.min(right_value)),
            (Some(left_value), None) => Some(left_value),
            (None, Some(right_value)) => Some(right_value),
            (None, None) => None,
        };
        self.maximum_solved_theta = match (self.maximum_solved_theta, other.maximum_solved_theta) {
            (Some(left_value), Some(right_value)) => Some(left_value.max(right_value)),
            (Some(left_value), None) => Some(left_value),
            (None, Some(right_value)) => Some(right_value),
            (None, None) => None,
        };

        if self.representative_transform_diagnostics.is_none() {
            self.representative_transform_diagnostics = other.representative_transform_diagnostics;
        }

        self
    }

    fn minimum_solved_theta(&self) -> f64 {
        self.minimum_solved_theta.unwrap_or(0.0)
    }

    fn maximum_solved_theta(&self) -> f64 {
        self.maximum_solved_theta.unwrap_or(0.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct TransitionProbabilityRow {
    probability_mass: Vec<f64>,
    representative_transform_diagnostics: Option<ExponentialAffineTransformIntegrationResult>,
}

fn transition_probability_row_from_shared_frequency_grid(
    transition_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    driver_parameters: NigBuySellDriverParameters,
    theta: f64,
    local_q_core_expected_increment: f64,
    target_boundaries: &[f64],
    enable_frequency_parallelism: bool,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<TransitionProbabilityRow, VanillaOptionPricingRunError> {
    if target_boundaries.is_empty() {
        return Ok(TransitionProbabilityRow {
            probability_mass: vec![1.0],
            representative_transform_diagnostics: None,
        });
    }

    let shift = transition_context.deviation_decay_factor * transition_row.boundary_state.deviation
        - local_q_core_expected_increment;

    let cdf_evaluator = SharedRiskNeutralCoreCdfEvaluator::new(
        transition_context,
        transition_row.boundary_state.deviation,
        driver_parameters,
        theta,
    )?;

    let first_boundary = target_boundaries[0] - shift;

    let boundary_step = if target_boundaries.len() <= 1 {
        0.0
    } else {
        target_boundaries[1] - target_boundaries[0]
    };

    let (projected_cdf_values, representative_transform_diagnostics) = cdf_evaluator
        .evaluate_uniform_boundaries(
            first_boundary,
            boundary_step,
            target_boundaries.len(),
            enable_frequency_parallelism,
            pricing_thread_pool,
            cpu_count,
        )?;

    let row = probability_mass_from_sampled_cdf_values(projected_cdf_values.as_slice());

    Ok(TransitionProbabilityRow {
        probability_mass: row,
        representative_transform_diagnostics,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SharedMidpointFrequencyGrid {
    integration_step: f64,
    integration_step_count: usize,
}

impl SharedMidpointFrequencyGrid {
    fn from_uniform_boundary_grid(boundary_step: f64, boundary_count: usize) -> Self {
        Self {
            integration_step: 2.0 * std::f64::consts::PI / (boundary_step * boundary_count as f64),
            integration_step_count: boundary_count,
        }
    }

    fn policy() -> &'static str {
        INTERNAL_FORWARD_LAW_FREQUENCY_GRID_POLICY
    }
}

struct SharedRiskNeutralCoreCdfEvaluator<'a> {
    transition_context: &'a LocalTransitionRequestContext,
    deviation: f64,
    driver_parameters: NigBuySellDriverParameters,
    theta: f64,
    denominator: ComplexScalar,
}

impl<'a> SharedRiskNeutralCoreCdfEvaluator<'a> {
    fn new(
        transition_context: &'a LocalTransitionRequestContext,
        deviation: f64,
        driver_parameters: NigBuySellDriverParameters,
        theta: f64,
    ) -> Result<Self, VanillaOptionPricingRunError> {
        let denominator =
            evaluate_core_log_return_characteristic_function_under_local_measure_with_parallelism(
                transition_context,
                driver_parameters,
                theta,
                ComplexScalar::new(0.0, 0.0),
                None,
                None,
            )?
            .value;

        Ok(Self {
            transition_context,
            deviation,
            driver_parameters,
            theta,
            denominator,
        })
    }

    fn evaluate_uniform_boundaries(
        &self,
        first_boundary: f64,
        boundary_step: f64,
        boundary_count: usize,
        enable_frequency_parallelism: bool,
        pricing_thread_pool: Option<&rayon::ThreadPool>,
        cpu_count: Option<usize>,
    ) -> Result<
        (
            Vec<f64>,
            Option<ExponentialAffineTransformIntegrationResult>,
        ),
        VanillaOptionPricingRunError,
    > {
        if boundary_count == 1 {
            let (cdf_value, representative_transform_diagnostics) =
                self.evaluate_single_boundary(first_boundary)?;
            return Ok((vec![cdf_value], representative_transform_diagnostics));
        }

        let frequency_grid =
            SharedMidpointFrequencyGrid::from_uniform_boundary_grid(boundary_step, boundary_count);
        debug_assert!(!SharedMidpointFrequencyGrid::policy().is_empty());

        let accumulation = if enable_frequency_parallelism
            && resolved_pricing_cpu_count(cpu_count) > 1
            && frequency_grid.integration_step_count > 1
        {
            install_pricing_parallelism(pricing_thread_pool, || {
                (0..frequency_grid.integration_step_count)
                    .into_par_iter()
                    .try_fold(
                        || BoundaryIntegralAccumulation::new(boundary_count),
                        |mut accumulation, frequency_index| {
                            let frequency =
                                (frequency_index as f64 + 0.5) * frequency_grid.integration_step;
                            let evaluation =
                                self.evaluate_frequency(ComplexScalar::new(frequency, 0.0))?;
                            accumulation.record_frequency(
                                first_boundary,
                                boundary_step,
                                frequency,
                                evaluation,
                            );
                            Ok::<_, VanillaOptionPricingRunError>(accumulation)
                        },
                    )
                    .try_reduce(
                        || BoundaryIntegralAccumulation::new(boundary_count),
                        |left, right| Ok::<_, VanillaOptionPricingRunError>(left.merge(right)),
                    )
            })?
        } else {
            let mut accumulation = BoundaryIntegralAccumulation::new(boundary_count);
            for frequency_index in 0..frequency_grid.integration_step_count {
                let frequency = (frequency_index as f64 + 0.5) * frequency_grid.integration_step;
                let evaluation = self.evaluate_frequency(ComplexScalar::new(frequency, 0.0))?;
                accumulation.record_frequency(first_boundary, boundary_step, frequency, evaluation);
            }
            accumulation
        };

        Ok((
            accumulation
                .boundary_integrals
                .into_iter()
                .map(|integral| {
                    (0.5 - frequency_grid.integration_step * integral / std::f64::consts::PI)
                        .clamp(0.0, 1.0)
                })
                .collect(),
            accumulation.representative_transform_diagnostics,
        ))
    }

    fn evaluate_single_boundary(
        &self,
        boundary: f64,
    ) -> Result<
        (f64, Option<ExponentialAffineTransformIntegrationResult>),
        VanillaOptionPricingRunError,
    > {
        let mut integration_upper_bound =
            initial_midpoint_upper_bound_from_phase_scale(boundary.abs());
        let initial_phase_resolution = (integration_upper_bound * boundary.abs()
            / std::f64::consts::PI)
            .ceil()
            .max(2.0) as usize;
        let mut integration_step_count = next_power_of_two_at_least(initial_phase_resolution)
            .ok_or_else(|| {
                VanillaOptionPricingRunError::InvalidObservedBoundaryStateDuringRecursion {
                    interval_index: self.transition_context.interval_index,
                    deviation: self.deviation,
                    message:
                        "single-boundary CDF refinement could not initialize a dyadic step count"
                            .to_string(),
                }
            })?;
        let absolute_cdf_budget = f64::EPSILON.cbrt();
        let mut representative_transform_diagnostics = None;

        loop {
            let coarse_statistics = self.evaluate_single_boundary_integral_statistics(
                boundary,
                integration_upper_bound,
                integration_step_count / 2,
                &mut representative_transform_diagnostics,
            )?;
            let full_statistics = self.evaluate_single_boundary_integral_statistics(
                boundary,
                integration_upper_bound,
                integration_step_count,
                &mut representative_transform_diagnostics,
            )?;

            let quadrature_error_estimate =
                (full_statistics.integral_estimate - coarse_statistics.integral_estimate).abs()
                    / 3.0;
            let tail_error_estimate = estimate_asymptotic_tail_integral_error(
                full_statistics.penultimate_frequency,
                full_statistics.penultimate_abs_integrand,
                full_statistics.terminal_frequency,
                full_statistics.terminal_abs_integrand,
            );

            if quadrature_error_estimate <= absolute_cdf_budget
                && tail_error_estimate <= absolute_cdf_budget
            {
                return Ok((
                    (0.5 - full_statistics.integration_step * full_statistics.integral_estimate
                        / std::f64::consts::PI)
                        .clamp(0.0, 1.0),
                    representative_transform_diagnostics,
                ));
            }

            if quadrature_error_estimate > absolute_cdf_budget {
                integration_step_count =
                    integration_step_count.checked_mul(2).ok_or_else(|| {
                        VanillaOptionPricingRunError::InvalidObservedBoundaryStateDuringRecursion {
                        interval_index: self.transition_context.interval_index,
                        deviation: self.deviation,
                        message:
                            "single-boundary CDF refinement overflowed during step-count doubling"
                                .to_string(),
                    }
                    })?;
                continue;
            }

            integration_upper_bound *= 2.0;
            integration_step_count = integration_step_count.checked_mul(2).ok_or_else(|| {
                VanillaOptionPricingRunError::InvalidObservedBoundaryStateDuringRecursion {
                    interval_index: self.transition_context.interval_index,
                    deviation: self.deviation,
                    message: "single-boundary CDF refinement overflowed during tail expansion"
                        .to_string(),
                }
            })?;
        }
    }

    fn evaluate_single_boundary_integral_statistics(
        &self,
        boundary: f64,
        integration_upper_bound: f64,
        integration_step_count: usize,
        representative_transform_diagnostics: &mut Option<
            ExponentialAffineTransformIntegrationResult,
        >,
    ) -> Result<SingleBoundaryIntegralStatistics, VanillaOptionPricingRunError> {
        let integration_step = integration_upper_bound / integration_step_count as f64;
        let mut integral_estimate = 0.0;
        let mut penultimate_frequency = 0.0;
        let mut penultimate_abs_integrand = 0.0;
        let mut terminal_frequency = 0.0;
        let mut terminal_abs_integrand = 0.0;

        for frequency_index in 0..integration_step_count {
            let frequency = (frequency_index as f64 + 0.5) * integration_step;
            let evaluation = self.evaluate_frequency(ComplexScalar::new(frequency, 0.0))?;
            if representative_transform_diagnostics.is_none() {
                *representative_transform_diagnostics = Some(evaluation.transform_diagnostics);
            }
            let boundary_phase =
                complex_exponential(ComplexScalar::new(0.0, -frequency * boundary));
            let integrand =
                complex_multiply(boundary_phase, evaluation.value).imaginary / frequency;
            let absolute_integrand = integrand.abs();

            integral_estimate += integrand;
            penultimate_frequency = terminal_frequency;
            penultimate_abs_integrand = terminal_abs_integrand;
            terminal_frequency = frequency;
            terminal_abs_integrand = absolute_integrand;
        }

        Ok(SingleBoundaryIntegralStatistics {
            integration_step,
            integral_estimate,
            penultimate_frequency,
            penultimate_abs_integrand,
            terminal_frequency,
            terminal_abs_integrand,
        })
    }

    fn evaluate_frequency(
        &self,
        argument: ComplexScalar,
    ) -> Result<PricingCharacteristicFunctionEvaluation, VanillaOptionPricingRunError> {
        let numerator =
            evaluate_core_log_return_characteristic_function_under_local_measure_with_parallelism(
                self.transition_context,
                self.driver_parameters,
                self.theta,
                argument,
                None,
                None,
            )?;

        let value = numerator.value.divide(self.denominator).ok_or(
            VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction {
                numerator: numerator.value,
                denominator: self.denominator,
            },
        )?;

        Ok(PricingCharacteristicFunctionEvaluation {
            value,
            transform_diagnostics: numerator.transform_diagnostics,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct BoundaryIntegralAccumulation {
    boundary_integrals: Vec<f64>,
    representative_transform_diagnostics: Option<ExponentialAffineTransformIntegrationResult>,
}

impl BoundaryIntegralAccumulation {
    fn new(boundary_count: usize) -> Self {
        Self {
            boundary_integrals: vec![0.0; boundary_count],
            representative_transform_diagnostics: None,
        }
    }

    fn record_frequency(
        &mut self,
        first_boundary: f64,
        boundary_step: f64,
        frequency: f64,
        evaluation: PricingCharacteristicFunctionEvaluation,
    ) {
        let step_phase = complex_exponential(ComplexScalar::new(0.0, -frequency * boundary_step));
        let mut boundary_phase =
            complex_exponential(ComplexScalar::new(0.0, -frequency * first_boundary));

        for boundary_integral in &mut self.boundary_integrals {
            let integrand = complex_multiply(boundary_phase, evaluation.value);
            *boundary_integral += integrand.imaginary / frequency;
            boundary_phase = complex_multiply(boundary_phase, step_phase);
        }

        if self.representative_transform_diagnostics.is_none() {
            self.representative_transform_diagnostics = Some(evaluation.transform_diagnostics);
        }
    }

    fn merge(mut self, other: Self) -> Self {
        for (left_integral, right_integral) in self
            .boundary_integrals
            .iter_mut()
            .zip(other.boundary_integrals)
        {
            *left_integral += right_integral;
        }

        if self.representative_transform_diagnostics.is_none() {
            self.representative_transform_diagnostics = other.representative_transform_diagnostics;
        }

        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SingleBoundaryIntegralStatistics {
    integration_step: f64,
    integral_estimate: f64,
    penultimate_frequency: f64,
    penultimate_abs_integrand: f64,
    terminal_frequency: f64,
    terminal_abs_integrand: f64,
}

fn pricing_request_for_strike(
    pricing_request: &VanillaOptionPricingRequest,
    runtime_interval_schedule: &PricingIntervalSchedule,
    strike: f64,
    maturity_interval_count: usize,
) -> Result<VanillaOptionPricingRequest, VanillaOptionPricingRunError> {
    let maturity_interval = runtime_interval_schedule
        .maturity_interval_hours(maturity_interval_count - 1)
        .map_err(|source| VanillaOptionPricingRunError::InvalidExecutionInput {
            source: super::runner::VanillaOptionPricingExecutionInputError::InvalidRuntimeIntervalSchedule {
                message: source.to_string(),
            },
        })?;

    VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(
            pricing_request.market_inputs.spot,
            strike,
            maturity_interval,
            pricing_request.market_inputs.risk_free_rate,
            pricing_request.market_inputs.dividend_yield,
        )
        .map_err(
            |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                source: super::runner::VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source:
                        super::contracts::VanillaOptionPricingRequestError::InvalidMarketInputs {
                            source,
                        },
                },
            },
        )?,
        pricing_request.payoff,
        pricing_request.boundary_input.clone(),
        pricing_request.model_parameters.clone(),
        pricing_request.interval_delta,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::InvalidExecutionInput {
            source: super::runner::VanillaOptionPricingExecutionInputError::InvalidRequest {
                source,
            },
        },
    )
}
