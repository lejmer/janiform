use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult;
use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};
use janiform_model_kernels::nig::NigBuySellDriverParameters;

use super::contracts::{
    AnchoredPricingRecursionDiagnostics, AnchoredPricingRecursionIntervalDiagnostics,
    VanillaOptionPayoffType, VanillaOptionPricingMarketInputs, VanillaOptionPricingRequest,
};
use super::implied_volatility::{
    VanillaOptionImpliedVolatilityInversionOutcome, invert_vanilla_option_implied_volatility,
};
use super::probability_projection::probability_mass_from_sampled_cdf_values;
use super::progress::PricingProgressEvent;
use super::progress_runtime::PricingProgressReporter;
use super::runner::{
    DeviationGrid, PricingCharacteristicFunctionEvaluation, RiskNeutralAnchorMaterialization,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingRunError, build_deviation_grid,
    build_local_boundary_input, build_local_pricing_request,
    build_pricing_risk_neutral_anchor_materialization, derive_local_tilted_interval_expectations,
    evaluate_core_log_return_characteristic_function_under_local_measure,
    resolve_local_transition_theta,
};
use super::thread_pool::{
    install_pricing_parallelism, resolved_pricing_cpu_count,
};
use super::time_scaling::intraday_year_fraction;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ForwardLawTerminalSnapshot {
    pub(crate) maturity_interval: f64,
    pub(crate) terminal_anchor_value: f64,
    pub(crate) deviation_grid: DeviationGrid,
    pub(crate) terminal_deviation_nodes: Vec<f64>,
    pub(crate) terminal_probability_mass: Vec<f64>,
    pub(crate) anchored_recursion_diagnostics: AnchoredPricingRecursionDiagnostics,
    pub(crate) representative_transform_diagnostics: Option<ExponentialAffineTransformIntegrationResult>,
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

pub(crate) fn solve_forward_law_scope(
    pricing_request: &VanillaOptionPricingRequest,
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
        interval_factors,
        driver_parameters,
        &mut None,
    )?;

    let mut unique_maturity_interval_counts = maturity_interval_counts.to_vec();

    unique_maturity_interval_counts.sort_unstable();
    unique_maturity_interval_counts.dedup();

    let coarse_grid = build_common_deviation_grid(
        pricing_request,
        strikes,
        &risk_neutral_anchor_materialization,
        unique_maturity_interval_counts.as_slice(),
        false,
    )?;
    let refined_grid = build_common_deviation_grid(
        pricing_request,
        strikes,
        &risk_neutral_anchor_materialization,
        unique_maturity_interval_counts.as_slice(),
        true,
    )?;

    let solved_snapshots = propagate_terminal_probability_mass(
        pricing_request,
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
                        == pricing_request.interval_delta * *requested_count as f64
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
    let current_observed_log_price = pricing_request.boundary_input.observed_boundary_state.observed_log_price;
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


    let price_strike = |strike: f64| {
        let price = discount_factor
            * terminal_snapshot
                .terminal_probability_mass
                .iter()
                .zip(terminal_snapshot.terminal_deviation_nodes.iter())
                .map(|(probability_mass, deviation)| {
                    let terminal_spot = pricing_request.market_inputs.spot
                        * (terminal_snapshot.terminal_anchor_value + *deviation
                            - current_observed_log_price)
                            .exp();
                    probability_mass * payoff_value(payoff_type, terminal_spot, strike)
                })
                .sum::<f64>();

        let implied_volatility_outcome = invert_vanilla_option_implied_volatility(
            VanillaOptionPricingMarketInputs::new(
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
    strikes: &[f64],
    risk_neutral_anchor_materialization: &RiskNeutralAnchorMaterialization,
    maturity_interval_counts: &[usize],
    refined: bool,
) -> Result<DeviationGrid, VanillaOptionPricingRunError> {
    let mut common_lower_bound = f64::INFINITY;
    let mut common_upper_bound = f64::NEG_INFINITY;
    let mut common_step = f64::INFINITY;

    for maturity_interval_count in maturity_interval_counts {
        for strike in strikes {
            let strike_request =
                pricing_request_for_strike(pricing_request, *strike, *maturity_interval_count)?;

            let strike_grid = build_deviation_grid(
                &strike_request,
                risk_neutral_anchor_materialization,
                *maturity_interval_count,
                refined,
            )?;

            let strike_step = if strike_grid.node_count <= 1 {
                0.0
            } else {
                (strike_grid.upper_bound - strike_grid.lower_bound)
                    / (strike_grid.node_count - 1) as f64
            };

            common_lower_bound = common_lower_bound.min(strike_grid.lower_bound);
            common_upper_bound = common_upper_bound.max(strike_grid.upper_bound);
            common_step = common_step.min(strike_step);
        }
    }

    if !common_lower_bound.is_finite()
        || !common_upper_bound.is_finite()
        || common_lower_bound >= common_upper_bound
        || !common_step.is_finite()
        || common_step <= 0.0
    {
        return Err(VanillaOptionPricingRunError::InvalidObservedBoundaryStateDuringRecursion {
            interval_index: pricing_request.boundary_input.interval_index,
            deviation: pricing_request.boundary_input.observed_boundary_state.deviation,
            message: "failed to derive a finite common deviation grid".to_string(),
        });
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
    let nodes = refined_grid.nodes();

    let initial_probability_mass = interpolate_initial_probability_mass(
        refined_grid,
        pricing_request.boundary_input.observed_boundary_state.deviation,
    );

    let frequency_grid =
        SharedMidpointFrequencyGrid::from_settings(pricing_request.resolved_fourier_inversion_settings());

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

        let transition_matrix = build_transition_matrix(
            pricing_request,
            interval_factor,
            driver_parameters,
            risk_neutral_anchor_materialization,
            refined_grid,
            interval_index,
            &frequency_grid,
            pricing_thread_pool,
            cpu_count,
            progress_reporter,
        )?;

        let mut next_probability_mass = vec![0.0; nodes.len()];

        for (row_index, row) in transition_matrix.rows.iter().enumerate() {
            let current_mass = current_probability_mass[row_index];
            for (column_index, probability_mass) in row.iter().enumerate() {
                next_probability_mass[column_index] += current_mass * probability_mass;
            }
        }

        current_probability_mass = next_probability_mass;

        interval_diagnostics.push(transition_matrix.interval_diagnostics);

        if representative_transform_diagnostics.is_none() {
            representative_transform_diagnostics =
                transition_matrix.representative_transform_diagnostics;
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
                maturity_interval: pricing_request.interval_delta * current_interval_count as f64,
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
struct IntervalTransitionMatrix {
    rows: Vec<Vec<f64>>,
    interval_diagnostics: AnchoredPricingRecursionIntervalDiagnostics,
    representative_transform_diagnostics: Option<ExponentialAffineTransformIntegrationResult>,
}

fn build_transition_matrix(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factor: VanillaOptionPricingIntervalFactors,
    driver_parameters: NigBuySellDriverParameters,
    risk_neutral_anchor_materialization: &RiskNeutralAnchorMaterialization,
    deviation_grid: DeviationGrid,
    interval_index: usize,
    frequency_grid: &SharedMidpointFrequencyGrid,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    progress_reporter: &PricingProgressReporter,
) -> Result<IntervalTransitionMatrix, VanillaOptionPricingRunError> {
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

    let solve_row = |deviation: f64| {
        let local_boundary_input = build_local_boundary_input(
            pricing_request,
            global_interval_index,
            expected_hawkes_state_before,
            current_anchor_value,
            current_pressure_state,
            deviation,
            interval_delta,
        )?;

        let local_request =
            build_local_pricing_request(pricing_request, local_boundary_input.clone(), interval_delta)?;

        let (local_transition_theta, _) =
            resolve_local_transition_theta(&local_request, interval_factor, driver_parameters)?;

        let local_expectations = derive_local_tilted_interval_expectations(
            &local_request,
            driver_parameters,
            interval_factor,
            local_transition_theta,
        )?;

        let row = transition_probability_row_from_shared_frequency_grid(
            &local_request,
            interval_factor,
            driver_parameters,
            local_transition_theta,
            local_expectations.core_expected_increment,
            &boundaries,
            frequency_grid,
        )?;

        let row_mass = row.probability_mass.iter().sum::<f64>();
        let maximum_tail_probability_mass = row.probability_mass[0]
            .max(row.probability_mass[row.probability_mass.len() - 1]);
        let probability_mass = row.probability_mass;

        Ok::<_, VanillaOptionPricingRunError>(SolvedTransitionRow {
            row: probability_mass,
            maximum_tail_probability_mass,
            row_probability_mass_error: (row_mass - 1.0).abs(),
            solved_theta: local_transition_theta,
            representative_transform_diagnostics: row.representative_transform_diagnostics,
        })
    };

    progress_reporter.start_transition_rows(
        interval_index,
        runtime_anchors.interval_deltas.len(),
        nodes.len(),
    );
    let row_solutions = if resolved_pricing_cpu_count(cpu_count) <= 1 {
        nodes
            .iter()
            .copied()
            .map(|deviation| {
                let row_solution = solve_row(deviation)?;
                progress_reporter.advance_transition_rows(1);
                Ok::<_, VanillaOptionPricingRunError>(row_solution)
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        install_pricing_parallelism(pricing_thread_pool, || {
            nodes
                .par_iter()
                .copied()
                .map(|deviation| {
                    let row_solution = solve_row(deviation)?;
                    progress_reporter.advance_transition_rows(1);
                    Ok::<_, VanillaOptionPricingRunError>(row_solution)
                })
                .collect::<Result<Vec<_>, _>>()
        })?
    };

    progress_reporter.finish_transition_rows();

    let maximum_tail_probability_mass = row_solutions
        .iter()
        .map(|row| row.maximum_tail_probability_mass)
        .fold(0.0_f64, f64::max);
    let maximum_row_probability_mass_error = row_solutions
        .iter()
        .map(|row| row.row_probability_mass_error)
        .fold(0.0_f64, f64::max);
    let minimum_solved_theta = row_solutions
        .iter()
        .map(|row| row.solved_theta)
        .min_by(|left, right| left.total_cmp(right))
        .unwrap_or(0.0);
    let maximum_solved_theta = row_solutions
        .iter()
        .map(|row| row.solved_theta)
        .max_by(|left, right| left.total_cmp(right))
        .unwrap_or(0.0);

    let interval_diagnostics = AnchoredPricingRecursionIntervalDiagnostics::try_new(
        global_interval_index,
        current_anchor_value,
        next_anchor_value,
        interval_delta,
        expected_hawkes_state_before,
        expected_hawkes_state_after,
        maximum_tail_probability_mass,
        maximum_row_probability_mass_error,
        minimum_solved_theta,
        maximum_solved_theta,
        Vec::new(),
    )
    .map_err(|source| VanillaOptionPricingRunError::InvalidAnchoredRecursionDiagnostics {
        source,
    })?;

    let representative_transform_diagnostics = row_solutions
        .iter()
        .find_map(|solution| solution.representative_transform_diagnostics);

    let rows = row_solutions
        .into_iter()
        .map(|solution| solution.row)
        .collect();

    Ok(IntervalTransitionMatrix {
        rows,
        interval_diagnostics,
        representative_transform_diagnostics,
    })
}

#[derive(Debug, Clone, PartialEq)]
struct SolvedTransitionRow {
    row: Vec<f64>,
    maximum_tail_probability_mass: f64,
    row_probability_mass_error: f64,
    solved_theta: f64,
    representative_transform_diagnostics: Option<ExponentialAffineTransformIntegrationResult>,
}

#[derive(Debug, Clone, PartialEq)]
struct TransitionProbabilityRow {
    probability_mass: Vec<f64>,
    representative_transform_diagnostics: Option<ExponentialAffineTransformIntegrationResult>,
}

fn transition_probability_row_from_shared_frequency_grid(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: VanillaOptionPricingIntervalFactors,
    driver_parameters: NigBuySellDriverParameters,
    theta: f64,
    local_q_core_expected_increment: f64,
    target_boundaries: &[f64],
    frequency_grid: &SharedMidpointFrequencyGrid,
) -> Result<TransitionProbabilityRow, VanillaOptionPricingRunError> {
    if target_boundaries.is_empty() {
        return Ok(TransitionProbabilityRow {
            probability_mass: vec![1.0],
            representative_transform_diagnostics: None,
        });
    }

    let deviation_decay_factor = pricing_request
        .boundary_input
        .deviation_decay_factor()
        .map_err(|source| VanillaOptionPricingRunError::InvalidExecutionInput {
            source: super::runner::VanillaOptionPricingExecutionInputError::InvalidRequest {
                source: super::contracts::VanillaOptionPricingRequestError::InvalidBoundaryInput {
                    source,
                },
            },
        })?;

    let news_intensity = interval_factors.news_intensity_input().map_err(|source| {
        VanillaOptionPricingRunError::InvalidExecutionInput {
            source: super::runner::VanillaOptionPricingExecutionInputError::InvalidIntervalFactors {
                source,
            },
        }
    })?;

    let shift = deviation_decay_factor
        * pricing_request.boundary_input.observed_boundary_state.deviation
        - local_q_core_expected_increment;

    let cdf_evaluator = SharedRiskNeutralCoreCdfEvaluator::new(
        pricing_request,
        driver_parameters,
        interval_factors,
        news_intensity,
        theta,
    )?;

    let first_boundary = target_boundaries[0] - shift;

    let boundary_step = if target_boundaries.len() <= 1 {
        0.0
    } else {
        target_boundaries[1] - target_boundaries[0]
    };

    let (projected_cdf_values, representative_transform_diagnostics) =
        cdf_evaluator.evaluate_uniform_boundaries(
        first_boundary,
        boundary_step,
        target_boundaries.len(),
        frequency_grid,
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
    fn from_settings(settings: super::fourier::FourierInversionEngineSettings) -> Self {
        Self {
            integration_step: settings.numeric_settings.integration_upper_bound
                / settings.numeric_settings.integration_step_count as f64,
            integration_step_count: settings.numeric_settings.integration_step_count,
        }
    }
}

struct SharedRiskNeutralCoreCdfEvaluator<'a> {
    pricing_request: &'a VanillaOptionPricingRequest,
    driver_parameters: NigBuySellDriverParameters,
    interval_factors: VanillaOptionPricingIntervalFactors,
    news_intensity: janiform_affine_oracle::affine_oracle::ExponentialAffineTransformNewsIntensityInput,
    theta: f64,
    denominator: ComplexScalar,
}

impl<'a> SharedRiskNeutralCoreCdfEvaluator<'a> {
    fn new(
        pricing_request: &'a VanillaOptionPricingRequest,
        driver_parameters: NigBuySellDriverParameters,
        interval_factors: VanillaOptionPricingIntervalFactors,
        news_intensity: janiform_affine_oracle::affine_oracle::ExponentialAffineTransformNewsIntensityInput,
        theta: f64,
    ) -> Result<Self, VanillaOptionPricingRunError> {
        let denominator = evaluate_core_log_return_characteristic_function_under_local_measure(
            pricing_request,
            driver_parameters,
            interval_factors,
            news_intensity,
            theta,
            ComplexScalar::new(0.0, 0.0),
        )?
        .value;

        Ok(Self {
            pricing_request,
            driver_parameters,
            interval_factors,
            news_intensity,
            theta,
            denominator,
        })
    }

    fn evaluate_uniform_boundaries(
        &self,
        first_boundary: f64,
        boundary_step: f64,
        boundary_count: usize,
        frequency_grid: &SharedMidpointFrequencyGrid,
    ) -> Result<
        (
            Vec<f64>,
            Option<ExponentialAffineTransformIntegrationResult>,
        ),
        VanillaOptionPricingRunError,
    > {
        let mut boundary_integrals = vec![0.0; boundary_count];
        let mut representative_transform_diagnostics = None;

        for frequency_index in 0..frequency_grid.integration_step_count {
            let frequency = (frequency_index as f64 + 0.5) * frequency_grid.integration_step;
            let evaluation = self.evaluate_frequency(
                ComplexScalar::new(frequency, 0.0),
                &mut representative_transform_diagnostics,
            )?;

            let step_phase =
                complex_exponential(ComplexScalar::new(0.0, -frequency * boundary_step));
            let mut boundary_phase =
                complex_exponential(ComplexScalar::new(0.0, -frequency * first_boundary));

            for boundary_integral in &mut boundary_integrals {
                let integrand = complex_multiply(boundary_phase, evaluation.value);
                *boundary_integral += integrand.imaginary / frequency;
                boundary_phase = complex_multiply(boundary_phase, step_phase);
            }
        }

        Ok((
            boundary_integrals
                .into_iter()
                .map(|integral| {
                    (0.5 - frequency_grid.integration_step * integral / std::f64::consts::PI)
                        .clamp(0.0, 1.0)
                })
                .collect(),
            representative_transform_diagnostics,
        ))
    }

    fn evaluate_frequency(
        &self,
        argument: ComplexScalar,
        representative_transform_diagnostics: &mut Option<ExponentialAffineTransformIntegrationResult>,
    ) -> Result<PricingCharacteristicFunctionEvaluation, VanillaOptionPricingRunError> {
        let numerator = evaluate_core_log_return_characteristic_function_under_local_measure(
            self.pricing_request,
            self.driver_parameters,
            self.interval_factors,
            self.news_intensity,
            self.theta,
            argument,
        )?;

        if representative_transform_diagnostics.is_none() {
            *representative_transform_diagnostics = Some(numerator.transform_diagnostics);
        }

        let value = numerator
            .value
            .divide(self.denominator)
            .ok_or(VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction {
                numerator: numerator.value,
                denominator: self.denominator,
            })?;

        Ok(PricingCharacteristicFunctionEvaluation {
            value,
            transform_diagnostics: numerator.transform_diagnostics,
        })
    }
}

fn pricing_request_for_strike(
    pricing_request: &VanillaOptionPricingRequest,
    strike: f64,
    maturity_interval_count: usize,
) -> Result<VanillaOptionPricingRequest, VanillaOptionPricingRunError> {
    let request = VanillaOptionPricingRequest::try_new_with_interval_delta(
        VanillaOptionPricingMarketInputs::try_new(
            pricing_request.market_inputs.spot,
            strike,
            pricing_request.interval_delta * maturity_interval_count as f64,
            pricing_request.market_inputs.risk_free_rate,
            pricing_request.market_inputs.dividend_yield,
        )
        .map_err(|source| VanillaOptionPricingRunError::InvalidExecutionInput {
            source: super::runner::VanillaOptionPricingExecutionInputError::InvalidRequest {
                source: super::contracts::VanillaOptionPricingRequestError::InvalidMarketInputs {
                    source,
                },
            },
        })?,
        pricing_request.payoff,
        pricing_request.boundary_input.clone(),
        pricing_request.model_parameters.clone(),
        pricing_request.interval_delta,
    )
    .map_err(|source| VanillaOptionPricingRunError::InvalidExecutionInput {
        source: super::runner::VanillaOptionPricingExecutionInputError::InvalidRequest { source },
    })?;

    Ok(
        if let Some(fourier_inversion_settings) = pricing_request.fourier_inversion_settings {
            request.with_fourier_inversion_settings(fourier_inversion_settings)
        } else {
            request
        },
    )
}

fn payoff_value(payoff_type: VanillaOptionPayoffType, terminal_spot: f64, strike: f64) -> f64 {
    match payoff_type {
        VanillaOptionPayoffType::Call => (terminal_spot - strike).max(0.0),
        VanillaOptionPayoffType::Put => (strike - terminal_spot).max(0.0),
    }
}
