use janiform_affine_oracle::endogenous_anchor::{
    EndogenousAnchorIntervalFactors, derive_expected_interval_projection_from_first_moment_dynamics,
};
use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsExpectedIntervalStatistics, SufficientStatisticsIntervalStatistics,
    SufficientStatisticsReturnComponents, SufficientStatisticsTransformSliceContext,
    SufficientStatisticsTransformSliceSamplingError, SufficientStatisticsTransformSlicesError,
    sample_interval_endogenous_transition_from_sampled_counts_with_randomness_factory,
    sample_interval_event_counts_from_expected_statistics_with_randomness_factory,
};
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessSubstreamDomain,
    SplitMix64SimulationRandomnessFactory,
};
use janiform_market_time::calendar::ExchangeCalendarRegistry;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::hawkes::target_source_rates_from_shared_activity_loading;
use janiform_model_kernels::long_variance::{
    LongVarianceParameters, LongVarianceState,
    stationary_return_energy_rate_per_hour_from_return_generator,
};
use janiform_model_kernels::market_pressure::{
    MarketPressureParameters, MarketPressureState, integrate_market_pressure_side_imbalance_mean,
    side_exposures_from_total_and_imbalance,
};
use janiform_model_kernels::news::expected_news_activity_loading_rate;
use janiform_model_kernels::nig::NigBuySellDriverParameters;
use janiform_model_kernels::return_clock::{
    IntegratedSideActivityDecomposition, compute_effective_return_clocks,
};
use janiform_model_kernels::return_decomposition::{ReturnDecomposition, ReturnDecompositionInput};

use crate::core::{
    IntervalOpenMarketSegments, SeededObservedState, SeededSimulationMarketStateResolver,
    build_interval_open_market_segments, derive_known_news_interval_slice_bounds,
    integrate_open_market_elapsed_hours,
};
use crate::long_variance::{
    apply_long_variance_event_count_jumps, apply_long_variance_return_energy_jump,
    return_energy_increment_from_generated_log_return, sample_long_variance_transition,
};
use crate::progress::{SimulationProgressEvent, SimulationProgressPhase, emit_progress_event};

use super::super::contracts::{
    CoarseSimulationPath, CoarseSimulationPathRecord, CoarseSimulationRunRequest,
};
use super::error::CoarseSimulationRunError;
use super::news::{
    apply_interval_news_terminal_contribution, sample_interval_direct_jump_totals,
    validate_known_news_factor_alignment,
};

/// Run deterministic coarse-interval simulation using the default splitmix64 randomness policy.
pub fn run_coarse_simulation(
    run_request: CoarseSimulationRunRequest,
    cancellation_token: CancellationToken,
) -> Result<CoarseSimulationPath, CoarseSimulationRunError> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;
    run_coarse_simulation_with_randomness_factory(
        run_request,
        cancellation_token,
        &randomness_factory,
    )
}

pub fn run_coarse_simulation_with_observer(
    run_request: CoarseSimulationRunRequest,
    cancellation_token: CancellationToken,
    observer: &mut dyn FnMut(SimulationProgressEvent),
) -> Result<CoarseSimulationPath, CoarseSimulationRunError> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;

    run_coarse_simulation_with_randomness_factory_and_observer(
        run_request,
        cancellation_token,
        &randomness_factory,
        observer,
    )
}

/// Run deterministic coarse-interval simulation with an injected randomness factory.
pub fn run_coarse_simulation_with_randomness_factory<F>(
    run_request: CoarseSimulationRunRequest,
    cancellation_token: CancellationToken,
    randomness_factory: &F,
) -> Result<CoarseSimulationPath, CoarseSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    run_coarse_simulation_with_randomness_factory_internal(
        run_request,
        cancellation_token,
        randomness_factory,
        None,
    )
}

pub fn run_coarse_simulation_with_randomness_factory_and_observer<F>(
    run_request: CoarseSimulationRunRequest,
    cancellation_token: CancellationToken,
    randomness_factory: &F,
    observer: &mut dyn FnMut(SimulationProgressEvent),
) -> Result<CoarseSimulationPath, CoarseSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    run_coarse_simulation_with_randomness_factory_internal(
        run_request,
        cancellation_token,
        randomness_factory,
        Some(observer),
    )
}

fn run_coarse_simulation_with_randomness_factory_internal<F>(
    run_request: CoarseSimulationRunRequest,
    cancellation_token: CancellationToken,
    randomness_factory: &F,
    mut observer: Option<&mut dyn FnMut(SimulationProgressEvent)>,
) -> Result<CoarseSimulationPath, CoarseSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    cancellation_token.check_cancelled()?;

    run_request
        .validate()
        .map_err(|source| CoarseSimulationRunError::InvalidRunRequest {
            source: Box::new(source),
        })?;

    let context = initialize_workflow_context(&run_request, randomness_factory)?;
    let mut state = initialize_workflow_state(&run_request);

    emit_progress_event(
        &mut observer,
        SimulationProgressEvent::new(
            SimulationProgressPhase::CoarseIntervals,
            0,
            run_request.horizon_interval_count,
            context.interval_delta_nanos,
            None,
        ),
    );

    for interval_index in 0..run_request.horizon_interval_count {
        cancellation_token.check_cancelled()?;
        process_coarse_interval(interval_index, &context, &mut state)?;

        emit_progress_event(
            &mut observer,
            SimulationProgressEvent::new(
                SimulationProgressPhase::CoarseIntervals,
                interval_index + 1,
                run_request.horizon_interval_count,
                context.interval_delta_nanos,
                None,
            ),
        );
    }

    cancellation_token.check_cancelled()?;

    CoarseSimulationPath::try_new(
        run_request.seed,
        run_request.start_time_utc_nanos,
        run_request.interval_delta,
        run_request.horizon_interval_count,
        run_request
            .model_inputs
            .sufficient_statistics_model_inputs
            .initial_excitation_state,
        state.terminal_state,
        state.records,
    )
    .map_err(|source| CoarseSimulationRunError::InvalidPathArtifact {
        source: Box::new(source),
    })
}

struct CoarseSimulationWorkflowContext<'a, F> {
    run_request: &'a CoarseSimulationRunRequest,
    calendar_registry: ExchangeCalendarRegistry,
    driver_parameters: NigBuySellDriverParameters,
    expected_news_severity_mark: f64,
    interval_delta_nanos: i64,
    interval_start_utc_nanos: Vec<i64>,
    interval_end_utc_nanos: Vec<i64>,
    interval_open_market_segments: Vec<IntervalOpenMarketSegments>,
    mean_reversion_kappa: f64,
    randomness_factory: &'a F,
}

struct CoarseSimulationWorkflowState {
    terminal_state: HawkesExcitationState,
    market_pressure_state: MarketPressureState,
    long_variance_state: LongVarianceState,
    observed_state: SeededObservedState,
    next_known_news_timestamp_position: usize,
    records: Vec<CoarseSimulationPathRecord>,
}

fn initialize_workflow_context<'a, F>(
    run_request: &'a CoarseSimulationRunRequest,
    randomness_factory: &'a F,
) -> Result<CoarseSimulationWorkflowContext<'a, F>, CoarseSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let model_inputs = &run_request.model_inputs;

    let calendar_registry = model_inputs.load_calendar_registry().map_err(|source| {
        CoarseSimulationRunError::CalendarRegistryLoadFailure {
            source: Box::new(source),
        }
    })?;

    let driver_parameters = model_inputs
        .sufficient_statistics_model_inputs
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .map_err(
            |source| CoarseSimulationRunError::InvalidNigDriverConfiguration {
                source: Box::new(source),
            },
        )?;

    let expected_news_severity_mark = model_inputs
        .sufficient_statistics_model_inputs
        .equation_specification
        .gamma_severity_mark_configuration
        .expected_severity_mark()
        .map_err(
            |source| CoarseSimulationRunError::EndogenousFirstMomentProjectionFailure {
                interval_index: 0,
                message: source.to_string(),
            },
        )?;

    let interval_delta_nanos = run_request.interval_delta_nanos().map_err(|source| {
        CoarseSimulationRunError::InvalidIntervalDelta {
            source: Box::new(source),
        }
    })?;

    let mean_reversion_kappa = model_inputs
        .sufficient_statistics_model_inputs
        .mean_reversion_fitted_parameters
        .kappa;

    let market_state_resolver =
        SeededSimulationMarketStateResolver::new(&calendar_registry, &model_inputs.exchange_code)
            .map_err(
            |source| CoarseSimulationRunError::OpenMarketExposureResolutionFailure {
                source: Box::new(source),
            },
        )?;

    let interval_start_utc_nanos = build_interval_start_times(run_request, interval_delta_nanos)?;

    let interval_end_utc_nanos =
        build_interval_end_times(interval_start_utc_nanos.as_slice(), interval_delta_nanos)?;

    let interval_open_market_segments = build_interval_open_market_segments(
        &market_state_resolver,
        interval_start_utc_nanos.as_slice(),
        interval_end_utc_nanos.as_slice(),
    )
    .map_err(
        |source| CoarseSimulationRunError::OpenMarketExposureResolutionFailure {
            source: Box::new(source),
        },
    )?;

    Ok(CoarseSimulationWorkflowContext {
        run_request,
        calendar_registry,
        driver_parameters,
        expected_news_severity_mark,
        interval_delta_nanos,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
        interval_open_market_segments,
        mean_reversion_kappa,
        randomness_factory,
    })
}

fn initialize_workflow_state(
    run_request: &CoarseSimulationRunRequest,
) -> CoarseSimulationWorkflowState {
    let model_inputs = &run_request.model_inputs;

    // The workflow state mutates independently from immutable model inputs.
    let initial_market_pressure_state = model_inputs
        .sufficient_statistics_model_inputs
        .initial_market_pressure_state
        .clone();

    CoarseSimulationWorkflowState {
        terminal_state: model_inputs
            .sufficient_statistics_model_inputs
            .initial_excitation_state,
        market_pressure_state: initial_market_pressure_state,
        long_variance_state: model_inputs.initial_long_variance_state,
        observed_state: SeededObservedState::from_boundary_state(
            &model_inputs
                .sufficient_statistics_model_inputs
                .initial_mean_reversion_boundary_state,
        ),
        next_known_news_timestamp_position: 0,
        records: Vec::with_capacity(run_request.horizon_interval_count),
    }
}

fn build_interval_start_times(
    run_request: &CoarseSimulationRunRequest,
    interval_delta_nanos: i64,
) -> Result<Vec<i64>, CoarseSimulationRunError> {
    (0..run_request.horizon_interval_count)
        .map(|interval_index| {
            run_request
                .start_time_utc_nanos
                .checked_add((interval_index as i64).saturating_mul(interval_delta_nanos))
                .ok_or(CoarseSimulationRunError::IntervalBoundaryOverflow { interval_index })
        })
        .collect::<Result<Vec<_>, _>>()
}

fn build_interval_end_times(
    interval_start_utc_nanos: &[i64],
    interval_delta_nanos: i64,
) -> Result<Vec<i64>, CoarseSimulationRunError> {
    interval_start_utc_nanos
        .iter()
        .copied()
        .enumerate()
        .map(|(interval_index, interval_start_utc_nanos)| {
            interval_start_utc_nanos
                .checked_add(interval_delta_nanos)
                .ok_or(CoarseSimulationRunError::IntervalBoundaryOverflow { interval_index })
        })
        .collect::<Result<Vec<_>, _>>()
}

fn process_coarse_interval<F>(
    interval_index: usize,
    context: &CoarseSimulationWorkflowContext<'_, F>,
    state: &mut CoarseSimulationWorkflowState,
) -> Result<(), CoarseSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let run_request = context.run_request;
    let model_inputs = &run_request.model_inputs;
    let interval_start_utc_nanos = context.interval_start_utc_nanos[interval_index];
    let interval_end_utc_nanos = context.interval_end_utc_nanos[interval_index];

    let endogenous_interval_factor_grids = EndogenousAnchorIntervalFactors::try_new(
        run_request.interval_factor_grids.seasonality_factor[interval_index],
        0.0,
        0.0,
        run_request.interval_delta,
    )
    .map_err(
        |source| CoarseSimulationRunError::EndogenousFirstMomentProjectionFailure {
            interval_index,
            message: source.to_string(),
        },
    )?;

    let first_moment_projection = derive_expected_interval_projection_from_first_moment_dynamics(
        interval_index,
        &model_inputs
            .sufficient_statistics_model_inputs
            .equation_specification,
        context.expected_news_severity_mark,
        endogenous_interval_factor_grids,
        state.terminal_state,
    )
    .map_err(
        |source| CoarseSimulationRunError::EndogenousFirstMomentProjectionFailure {
            interval_index,
            message: source.to_string(),
        },
    )?;

    let interval_known_news_timestamp_slice =
        derive_interval_known_news_slice(interval_index, context, state);

    validate_known_news_factor_alignment(
        interval_index,
        run_request.interval_delta,
        run_request.interval_factor_grids.known_news_intensity[interval_index],
        interval_known_news_timestamp_slice.len(),
    )?;

    let interval_direct_jump_totals = sample_interval_direct_jump_totals(
        run_request,
        &context.calendar_registry,
        interval_index,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
        interval_known_news_timestamp_slice,
        context.randomness_factory,
    )?;

    let transition_market_pressure_state = state.market_pressure_state.clone();

    let pressure_adjusted_expected_statistics = adjust_expected_statistics_for_market_pressure(
        first_moment_projection.expected_statistics,
        &model_inputs
            .sufficient_statistics_model_inputs
            .equation_specification
            .market_pressure_parameters,
        &transition_market_pressure_state,
        run_request.interval_delta,
        interval_index,
    )?;

    let interval_statistics_seed =
        janiform_foundations::randomness::derive_simulation_substream_seed(
            run_request.seed,
            SimulationRandomnessSubstreamDomain::CoarseStatisticsSampling,
            interval_index as u64,
        );

    let sampled_event_counts =
        sample_interval_event_counts_from_expected_statistics_with_randomness_factory(
            interval_statistics_seed,
            interval_index,
            pressure_adjusted_expected_statistics,
            context.randomness_factory,
        )
        .map_err(
            |source| CoarseSimulationRunError::TransformSliceSamplingFailure {
                interval_index,
                source: Box::new(source),
            },
        )?;

    let transition_context = SufficientStatisticsTransformSliceContext::try_new(
        interval_index,
        run_request.interval_factor_grids.seasonality_factor[interval_index],
        run_request
            .interval_factor_grids
            .news_intensity_input_at(interval_index),
        run_request.interval_delta,
        state.terminal_state,
        transition_market_pressure_state,
    )
    .map_err(
        |source| CoarseSimulationRunError::TransformSliceSamplingFailure {
            interval_index,
            source: Box::new(
                SufficientStatisticsTransformSliceSamplingError::InvalidSlices {
                    source: SufficientStatisticsTransformSlicesError::InvalidContext { source },
                },
            ),
        },
    )?;

    let sampled_transition =
        sample_interval_endogenous_transition_from_sampled_counts_with_randomness_factory(
            interval_statistics_seed,
            interval_index,
            &model_inputs
                .sufficient_statistics_model_inputs
                .equation_specification,
            transition_context,
            sampled_event_counts,
            context.randomness_factory,
        )
        .map_err(
            |source| CoarseSimulationRunError::TransformSliceSamplingFailure {
                interval_index,
                source: Box::new(source),
            },
        )?;

    let interval_terminal_state = apply_interval_news_terminal_contribution(
        interval_index,
        sampled_transition.terminal_excitation_state,
        interval_direct_jump_totals.terminal_hawkes_state_contribution,
    )?;

    // Sufficient-statistics records own the sampled terminal state.
    let interval_market_pressure_state = sampled_transition.terminal_market_pressure_state.clone();

    let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(
        pressure_adjusted_expected_statistics.expected_baseline_a_plus,
        pressure_adjusted_expected_statistics.expected_endogenous_a_plus,
        pressure_adjusted_expected_statistics.expected_baseline_a_minus,
        pressure_adjusted_expected_statistics.expected_endogenous_a_minus,
        sampled_event_counts.n_plus,
        sampled_event_counts.n_minus,
        interval_terminal_state,
        sampled_transition.terminal_market_pressure_state,
        sampled_transition.market_pressure_anchor_increment,
    )
    .map_err(
        |source| CoarseSimulationRunError::TransformSliceSamplingFailure {
            interval_index,
            source: Box::new(
                SufficientStatisticsTransformSliceSamplingError::InvalidSampledStatistics {
                    interval_index,
                    source,
                },
            ),
        },
    )?;

    let sampled_n_plus = sampled_statistics.n_plus;
    let sampled_n_minus = sampled_statistics.n_minus;

    let record_output = build_coarse_interval_path_record(
        interval_index,
        context,
        state,
        sampled_statistics,
        interval_direct_jump_totals.open_jump_return,
        interval_direct_jump_totals.closed_gap_return,
    )?;

    state.records.push(record_output.path_record);

    state.terminal_state = interval_terminal_state;
    state.market_pressure_state = interval_market_pressure_state;

    state.long_variance_state = apply_long_variance_event_count_jumps(
        record_output.terminal_long_variance_state,
        model_inputs.long_variance_parameters,
        sampled_n_plus,
        sampled_n_minus,
    )
    .map_err(
        |source| CoarseSimulationRunError::LongVarianceTransitionFailure {
            interval_index,
            source: Box::new(source),
        },
    )?;

    Ok(())
}

fn adjust_expected_statistics_for_market_pressure(
    expected_statistics: SufficientStatisticsExpectedIntervalStatistics,
    market_pressure_parameters: &MarketPressureParameters,
    market_pressure_state: &MarketPressureState,
    interval_delta: f64,
    interval_index: usize,
) -> Result<SufficientStatisticsExpectedIntervalStatistics, CoarseSimulationRunError> {
    let integrated_side_imbalance = integrate_market_pressure_side_imbalance_mean(
        market_pressure_parameters,
        market_pressure_state,
        interval_delta,
    )
    .map_err(
        |source| CoarseSimulationRunError::MarketPressureTransitionFailure {
            interval_index,
            source: Box::new(source),
        },
    )?;

    let average_side_imbalance = if interval_delta > 0.0 {
        integrated_side_imbalance / interval_delta
    } else {
        market_pressure_state
            .expected_side_imbalance(market_pressure_parameters)
            .map_err(
                |source| CoarseSimulationRunError::MarketPressureTransitionFailure {
                    interval_index,
                    source: Box::new(source),
                },
            )?
    };

    let total_baseline_activity = expected_statistics.expected_baseline_a_plus
        + expected_statistics.expected_baseline_a_minus;

    let (expected_baseline_a_plus, expected_baseline_a_minus) =
        side_exposures_from_total_and_imbalance(total_baseline_activity, average_side_imbalance)
            .map_err(
                |source| CoarseSimulationRunError::MarketPressureTransitionFailure {
                    interval_index,
                    source: Box::new(source),
                },
            )?;

    let expected_n_plus = expected_baseline_a_plus + expected_statistics.expected_endogenous_a_plus;
    let expected_n_minus =
        expected_baseline_a_minus + expected_statistics.expected_endogenous_a_minus;

    SufficientStatisticsExpectedIntervalStatistics::try_new(
        expected_baseline_a_plus,
        expected_statistics.expected_endogenous_a_plus,
        expected_baseline_a_minus,
        expected_statistics.expected_endogenous_a_minus,
        expected_n_plus,
        expected_n_minus,
    )
    .map_err(
        |source| CoarseSimulationRunError::TransformSliceSamplingFailure {
            interval_index,
            source: Box::new(
                SufficientStatisticsTransformSliceSamplingError::InvalidSampledStatistics {
                    interval_index,
                    source,
                },
            ),
        },
    )
}

fn derive_interval_known_news_slice<'a, F>(
    interval_index: usize,
    context: &CoarseSimulationWorkflowContext<'a, F>,
    state: &mut CoarseSimulationWorkflowState,
) -> &'a [i64]
where
    F: SimulationRandomnessFactory,
{
    let known_news_timestamps = &context
        .run_request
        .model_inputs
        .news_process_configuration
        .arrivals
        .known
        .scheduled_event_timestamps_utc_nanos;

    let (
        interval_known_news_start_position,
        interval_known_news_end_position,
        next_interval_known_news_timestamp_position,
    ) = derive_known_news_interval_slice_bounds(
        known_news_timestamps,
        state.next_known_news_timestamp_position,
        context.interval_start_utc_nanos[interval_index],
        context.interval_end_utc_nanos[interval_index],
    );

    state.next_known_news_timestamp_position = next_interval_known_news_timestamp_position;

    &known_news_timestamps[interval_known_news_start_position..interval_known_news_end_position]
}

struct CoarseIntervalRecordBuildOutput {
    path_record: CoarseSimulationPathRecord,
    terminal_long_variance_state: LongVarianceState,
}

fn build_coarse_interval_path_record<F>(
    interval_index: usize,
    context: &CoarseSimulationWorkflowContext<'_, F>,
    state: &mut CoarseSimulationWorkflowState,
    sampled_statistics: SufficientStatisticsIntervalStatistics,
    open_jump_return: f64,
    closed_gap_return: f64,
) -> Result<CoarseIntervalRecordBuildOutput, CoarseSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let run_request = context.run_request;
    let model_inputs = &run_request.model_inputs;

    let interval_start_utc_nanos = context.interval_start_utc_nanos[interval_index];
    let interval_end_utc_nanos = context.interval_end_utc_nanos[interval_index];

    let mut interval_return_stream = context.randomness_factory.create_substream(
        run_request.seed,
        SimulationRandomnessSubstreamDomain::CoarseTradingTimeReturn,
        interval_index as u64,
    );

    let mut long_variance_stream = context.randomness_factory.create_substream(
        run_request.seed,
        SimulationRandomnessSubstreamDomain::LongVarianceDiffusion,
        interval_index as u64,
    );

    let long_variance_stationary_mean_reversion_level =
        coarse_interval_long_variance_stationary_mean_reversion_level(run_request, interval_index)?;

    let long_variance_transition = sample_long_variance_transition(
        state.long_variance_state,
        model_inputs.long_variance_parameters,
        long_variance_stationary_mean_reversion_level,
        run_request.interval_delta,
        &mut long_variance_stream,
    )
    .map_err(
        |source| CoarseSimulationRunError::LongVarianceTransitionFailure {
            interval_index,
            source: Box::new(source),
        },
    )?;

    let effective_return_clocks = compute_effective_return_clocks(
        model_inputs.return_clock_exposure_parameters,
        IntegratedSideActivityDecomposition::new(
            sampled_statistics.baseline_a_plus * long_variance_transition.baseline_clock_scale,
            sampled_statistics.endogenous_a_plus,
        ),
        IntegratedSideActivityDecomposition::new(
            sampled_statistics.baseline_a_minus * long_variance_transition.baseline_clock_scale,
            sampled_statistics.endogenous_a_minus,
        ),
    )
    .map_err(
        |source| CoarseSimulationRunError::EffectiveReturnClockBuildFailure {
            interval_index,
            source: Box::new(source),
        },
    )?;

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: effective_return_clocks.buy_return_clock,
        a_minus: effective_return_clocks.sell_return_clock,
        open_jump: open_jump_return,
        closed_gap: closed_gap_return,
    })
    .map_err(
        |source| CoarseSimulationRunError::InvalidReturnDecomposition {
            interval_index,
            source: Box::new(source),
        },
    )?;

    let trading_time_contributions = return_decomposition
        .sample_nig_trading_time_increment(context.driver_parameters, &mut interval_return_stream)
        .map_err(
            |source| CoarseSimulationRunError::TradingTimeSamplingFailure {
                interval_index,
                source: Box::new(source),
            },
        )?;

    let buy_side_return = trading_time_contributions.buy_side_increment;
    let sell_side_return = -trading_time_contributions.sell_side_increment;
    let trading_time_return = trading_time_contributions.combined_increment;
    let raw_total_return = trading_time_return + open_jump_return + closed_gap_return;
    let return_energy_increment =
        return_energy_increment_from_generated_log_return(raw_total_return).map_err(|source| {
            CoarseSimulationRunError::LongVarianceTransitionFailure {
                interval_index,
                source: Box::new(source),
            }
        })?;

    let terminal_long_variance_state = apply_long_variance_return_energy_jump(
        long_variance_transition.terminal_state,
        model_inputs.long_variance_parameters,
        return_energy_increment,
    )
    .map_err(
        |source| CoarseSimulationRunError::LongVarianceTransitionFailure {
            interval_index,
            source: Box::new(source),
        },
    )?;

    let previous_observed_log_price = state.observed_state.observed_log_price;

    let elapsed_open_hours = integrate_open_market_elapsed_hours(
        context.interval_open_market_segments[interval_index].as_slice(),
        interval_start_utc_nanos,
        interval_end_utc_nanos,
    );

    let projected_core_increment = state
        .observed_state
        .project_core_increment(
            context.mean_reversion_kappa,
            elapsed_open_hours,
            raw_total_return,
            model_inputs.observed_price_tick_size,
            interval_index,
        )
        .map_err(
            |source| CoarseSimulationRunError::TickGridProjectionFailure {
                interval_index,
                source: Box::new(source),
            },
        )?;

    let total_return = projected_core_increment.observed_core_increment;

    let return_components = SufficientStatisticsReturnComponents::try_new(
        buy_side_return,
        sell_side_return,
        trading_time_return + total_return - raw_total_return,
        open_jump_return,
        closed_gap_return,
        total_return,
    )
    .map_err(|source| CoarseSimulationRunError::InvalidReturnComponents {
        interval_index,
        source: Box::new(source),
    })?;

    let updated_observed_state = state
        .observed_state
        .apply_projected_core_increment(
            context.mean_reversion_kappa,
            elapsed_open_hours,
            projected_core_increment,
        )
        .apply_anchor_reset(
            model_inputs
                .sufficient_statistics_model_inputs
                .mean_reversion_runtime_interval_anchors
                .end_anchor_values[interval_index],
        );

    let observed_interval_return =
        updated_observed_state.observed_log_price - previous_observed_log_price;

    let path_record = CoarseSimulationPathRecord::try_new(
        interval_index,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
        sampled_statistics,
        return_components,
        total_return,
        observed_interval_return,
        updated_observed_state.observed_log_price,
        updated_observed_state.anchor_value,
        updated_observed_state.deviation_from_anchor,
    )
    .map_err(|source| CoarseSimulationRunError::InvalidPathRecord {
        interval_index,
        source: Box::new(source),
    })?;

    state.observed_state = updated_observed_state;

    Ok(CoarseIntervalRecordBuildOutput {
        path_record,
        terminal_long_variance_state,
    })
}

fn coarse_interval_long_variance_stationary_mean_reversion_level(
    run_request: &CoarseSimulationRunRequest,
    interval_index: usize,
) -> Result<f64, CoarseSimulationRunError> {
    let equation_specification = &run_request
        .model_inputs
        .sufficient_statistics_model_inputs
        .equation_specification;

    let news_process_configuration = &run_request.model_inputs.news_process_configuration;

    let news_activity_loading_rate = expected_news_activity_loading_rate(
        run_request.interval_factor_grids.known_news_intensity[interval_index],
        run_request.interval_factor_grids.surprise_news_intensity[interval_index],
        news_process_configuration.known_component_mark_scales,
        news_process_configuration.surprise_component_mark_scales,
        news_process_configuration
            .shared_mark_law
            .gamma_severity_marks,
    )
    .map_err(
        |source| CoarseSimulationRunError::InvalidNewsActivityLoadingRate {
            interval_index,
            source: Box::new(source),
        },
    )?;

    let exogenous_source_rates = target_source_rates_from_shared_activity_loading(
        equation_specification.hawkes_kernel_parameters.rho,
        news_activity_loading_rate,
    )
    .ok_or(CoarseSimulationRunError::InvalidNewsTargetSourceRates { interval_index })?;

    let stationary_return_energy_rate_per_hour =
        stationary_return_energy_rate_per_hour_from_return_generator(
            equation_specification.hawkes_kernel_parameters,
            exogenous_source_rates,
            run_request.model_inputs.return_clock_exposure_parameters,
            run_request
                .model_inputs
                .sufficient_statistics_model_inputs
                .nig_shared_plus_shift_configuration
                .derive_driver_parameters()
                .map_err(
                    |source| CoarseSimulationRunError::InvalidNigDriverConfiguration {
                        source: Box::new(source),
                    },
                )?,
        )
        .ok_or(
            CoarseSimulationRunError::InvalidLongVarianceStationaryEnergyRate { interval_index },
        )?;

    let long_variance_parameters = LongVarianceParameters::try_new(
        run_request
            .model_inputs
            .long_variance_parameters
            .mean_reversion_rate,
        run_request.model_inputs.long_variance_parameters.vol_of_vol,
        stationary_return_energy_rate_per_hour,
        run_request
            .model_inputs
            .long_variance_parameters
            .return_energy_loading,
        run_request
            .model_inputs
            .long_variance_parameters
            .buy_event_loading,
        run_request
            .model_inputs
            .long_variance_parameters
            .sell_event_loading,
    )
    .map_err(
        |source| CoarseSimulationRunError::InvalidLongVarianceParameters {
            source: Box::new(source),
        },
    )?;

    long_variance_parameters
        .stationary_mean_reversion_level_for_hawkes_kernel(
            equation_specification.hawkes_kernel_parameters,
            exogenous_source_rates,
        )
        .map_err(
            |source| CoarseSimulationRunError::InvalidLongVarianceStationaryLevel {
                source: Box::new(source),
            },
        )
}

#[cfg(test)]
mod tests {
    use super::{CoarseSimulationRunError, run_coarse_simulation as run_coarse_simulation_core};
    use crate::CoarseSimulationPath;
    use crate::coarse::{CoarseSimulationModelInputs, CoarseSimulationRunRequest};
    use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
    use janiform_affine_oracle::sufficient_statistics::{
        SufficientStatisticsIntervalFactors, SufficientStatisticsModelInputs,
    };
    use janiform_foundations::cancellation::CancellationToken;
    use janiform_market_time::calendar::{
        CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarRegistryArtifact,
        RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
    };
    use janiform_model_kernels::hawkes::{
        HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
    };
    use janiform_model_kernels::long_variance::{LongVarianceParameters, LongVarianceState};
    use janiform_model_kernels::market_pressure::MarketPressureParameters;
    use janiform_model_kernels::mean_reversion::{
        MeanReversionBoundaryState, MeanReversionFittedParameters,
        MeanReversionRuntimeIntervalAnchors,
    };
    use janiform_model_kernels::news::{
        GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
        NewsComponentMarkScaleConfiguration, NewsProcessConfiguration,
        SharedNewsMarkLawConfiguration, SurpriseNewsArrivalConfiguration,
    };
    use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

    fn run_coarse_simulation(
        run_request: CoarseSimulationRunRequest,
    ) -> Result<CoarseSimulationPath, CoarseSimulationRunError> {
        run_coarse_simulation_core(run_request, CancellationToken::uncancelled())
    }

    #[test]
    fn coarse_simulation_does_not_decay_mean_reversion_during_closed_interval() {
        let equation_specification = ExponentialAffineTransformEquationSpecification::try_new(
            HawkesKernelParameters::try_new(
                0.8,
                1.6,
                HawkesKernelLoadings {
                    plus_plus: 0.0,
                    plus_minus: 0.0,
                    minus_plus: 0.0,
                    minus_minus: 0.0,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.0,
                    plus_minus: 0.0,
                    minus_plus: 0.0,
                    minus_minus: 0.0,
                },
            )
            .expect("kernel parameters should construct"),
            GammaSeverityMarkConfiguration::try_new(1.8).expect("gamma marks should construct"),
            LongVarianceParameters::neutral(),
            MarketPressureParameters::no_anchor_impact(),
        )
        .expect("equation specification should construct");

        let sufficient_statistics_model_inputs =
            SufficientStatisticsModelInputs::try_new(SufficientStatisticsModelInputs {
                equation_specification,
                nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration::try_new(
                    2.3, -0.8, 0.7, 0.1, -0.2,
                )
                .expect("nig configuration should construct"),
                initial_excitation_state: HawkesExcitationState {
                    y_plus_plus: 0.0,
                    y_plus_minus: 0.0,
                    y_minus_plus: 0.0,
                    y_minus_minus: 0.0,
                },
                initial_market_pressure_state:
                    janiform_model_kernels::market_pressure::MarketPressureState::neutral(),
                long_variance_parameters: LongVarianceParameters::neutral(),
                initial_long_variance_state: LongVarianceState::unit(),
                mean_reversion_fitted_parameters: MeanReversionFittedParameters::try_new(1.0)
                    .expect("positive-kappa mean reversion should construct"),
                mean_reversion_runtime_interval_anchors:
                    MeanReversionRuntimeIntervalAnchors::try_new(
                        "explicit_runtime_interval_grid_alignment.v1",
                        2.0,
                        vec![2.0],
                        vec![1.0],
                    )
                    .expect("mean-reversion runtime anchors should construct"),
                initial_mean_reversion_boundary_state: MeanReversionBoundaryState::try_new(
                    3.0, 2.0, 1.0,
                )
                .expect("mean-reversion boundary state should construct"),
            })
            .expect("sufficient-statistics model inputs should construct");

        let model_inputs = CoarseSimulationModelInputs::try_new(CoarseSimulationModelInputs {
            sufficient_statistics_model_inputs,
            news_process_configuration: NewsProcessConfiguration::try_new(
                NewsArrivalConfiguration {
                    known: KnownNewsArrivalConfiguration {
                        scheduled_event_timestamps_utc_nanos: vec![],
                    },
                    surprise: SurpriseNewsArrivalConfiguration {
                        open_baseline_intensity: 0.0,
                        closed_baseline_intensity: 0.0,
                    },
                },
                SharedNewsMarkLawConfiguration {
                    gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.4)
                        .expect("gamma marks should construct"),
                },
                NewsComponentMarkScaleConfiguration {
                    activity_coupling_scale: 0.7,
                    return_jump_drift: 0.0,
                    return_jump_volatility: 0.0,
                },
                NewsComponentMarkScaleConfiguration {
                    activity_coupling_scale: 0.5,
                    return_jump_drift: 0.0,
                    return_jump_volatility: 0.0,
                },
            )
            .expect("news process configuration should construct"),
            return_clock_exposure_parameters:
                janiform_model_kernels::return_clock::ReturnClockExposureParameters::neutral(),
            long_variance_parameters: LongVarianceParameters::neutral(),
            initial_long_variance_state: LongVarianceState::unit(),
            observed_price_tick_size: None,
            timestamp_batching_surface:
                janiform_model_kernels::timestamp_batching_surface::TimestampBatchingSurface::disabled(
                ),
            calendar_registry_artifact: ExchangeCalendarRegistryArtifact {
                schema_version: "calendar.registry.v1".to_string(),
                artifact_date: "2024-01-02".to_string(),
                exchanges: vec![ExchangeCalendarArtifact {
                    schema_version: "calendar.v1".to_string(),
                    exchange_code: "XNYS".to_string(),
                    exchange_name: "New York Stock Exchange".to_string(),
                    timezone_name: Some("America/New_York".to_string()),
                    rule_metadata: CalendarRuleMetadataArtifact {
                        holiday_calendar_code: "US_EQUITIES".to_string(),
                        early_close_rule_set: None,
                    },
                    regular_sessions: vec![RegularSessionArtifact {
                        weekday: TradingWeekday::Monday,
                        windows: vec![SessionWindowArtifact {
                            label: "regular".to_string(),
                            open_time: "09:30".to_string(),
                            close_time: "16:00".to_string(),
                        }],
                    }],
                    holiday_dates: vec![],
                    early_close_sessions: vec![],
                }],
            },
            exchange_code: "XNYS".to_string(),
        })
        .expect("coarse model inputs should construct");

        let run_request = CoarseSimulationRunRequest::try_new(
            17,
            1_705_930_200_000_000_000,
            1,
            1.0,
            model_inputs,
            SufficientStatisticsIntervalFactors::try_new(
                vec![0.0],
                vec![0.0],
                vec![0.0],
                Some(1.0),
            )
            .expect("interval factors should construct"),
        )
        .expect("coarse simulation request should construct");

        let coarse_path = run_coarse_simulation(run_request).expect("coarse simulation should run");

        assert_eq!(coarse_path.records.len(), 1);
        assert_eq!(coarse_path.records[0].observed_interval_return, 0.0);
        assert_eq!(coarse_path.records[0].observed_log_price_end, 3.0);
        assert_eq!(coarse_path.records[0].deviation_from_anchor_end, 1.0);
    }
}
