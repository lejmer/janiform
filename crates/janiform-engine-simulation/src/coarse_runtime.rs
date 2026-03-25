use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::risk::{
    SufficientStatisticsBoundaryStateUpdateError, SufficientStatisticsBoundaryStateUpdateSnapshot,
    SufficientStatisticsRiskReturnComponents, SufficientStatisticsRiskReturnComponentsError,
    SufficientStatisticsTransformSliceContext, SufficientStatisticsTransformSliceSamplingError,
    SufficientStatisticsTransformSliceSettings, SufficientStatisticsTransformSliceSolveError,
    sample_interval_statistics_from_transform_slices_with_randomness_factory,
    solve_sufficient_statistics_transform_slices, update_boundary_state_from_interval_statistics,
};
use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformNewsIntensityInput;
use janiform_foundations::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessSubstreamDomain,
    SplitMix64SimulationRandomnessFactory,
};
use janiform_model_kernels::hawkes::{
    HawkesTransitionError, HawkesTransitionStep, evolve_hawkes_state,
};
use janiform_model_kernels::mean_reversion::evolve_mean_reversion_boundary_state_on_interval_grid;
use janiform_model_kernels::news::{
    NewsArrivalComponent, NewsMarkInterfaceError, NewsProcessConfigurationError,
    SharedNewsAccumulatorState, SharedNewsEventDispatchError, SharedNewsEventTimestampedInput,
    dispatch_shared_news_events_with_resolved_market_states,
};
use janiform_model_kernels::return_decomposition::{ReturnDecomposition, ReturnDecompositionInput};

use super::coarse::{CoarseSimulationPath, CoarseSimulationPathError, CoarseSimulationPathRecord};
use super::runtime::{
    build_timestamped_news_event_input, derive_known_news_interval_slice_bounds,
    sample_count_from_expectation, sample_interval_timestamp_utc_nanos,
};
use super::{CoarseSimulationRunRequest, CoarseSimulationRunRequestError};

const KNOWN_NEWS_INTENSITY_ALIGNMENT_TOLERANCE: f64 = 1.0e-12;
const TARGET_TRANSFORM_PROBE_SHIFT: f64 = 0.02;
const MIN_TRANSFORM_PROBE_DISTANCE: f64 = 1.0e-8;

/// Run deterministic coarse-interval simulation using the default splitmix64 randomness policy.
pub fn run_coarse_simulation(
    run_request: CoarseSimulationRunRequest,
) -> Result<CoarseSimulationPath, CoarseSimulationRunError> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;
    run_coarse_simulation_with_randomness_factory(run_request, &randomness_factory)
}

/// Run deterministic coarse-interval simulation with an injected randomness factory.
pub fn run_coarse_simulation_with_randomness_factory<F>(
    run_request: CoarseSimulationRunRequest,
    randomness_factory: &F,
) -> Result<CoarseSimulationPath, CoarseSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    run_request
        .validate()
        .map_err(|source| CoarseSimulationRunError::InvalidRunRequest { source })?;

    let model_inputs = &run_request.model_inputs;

    let calendar_registry = model_inputs
        .load_calendar_registry()
        .map_err(|source| CoarseSimulationRunError::CalendarRegistryLoadFailure { source })?;

    let kernel_parameters = model_inputs
        .risk_model_inputs
        .equation_specification
        .hawkes_kernel_parameters;
    let driver_parameters = model_inputs
        .risk_model_inputs
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .map_err(|source| CoarseSimulationRunError::InvalidNigDriverConfiguration { source })?;

    let interval_delta_nanos = run_request
        .interval_delta_nanos()
        .map_err(|source| CoarseSimulationRunError::InvalidIntervalDelta { source })?;

    let mut terminal_state = model_inputs.risk_model_inputs.initial_excitation_state;

    let mut observed_boundary_state = model_inputs
        .risk_model_inputs
        .initial_mean_reversion_boundary_state
        .clone();

    let mut next_known_news_timestamp_position = 0usize;

    let mut records: Vec<CoarseSimulationPathRecord> =
        Vec::with_capacity(run_request.horizon_interval_count);

    let mean_reversion_kappa = model_inputs
        .risk_model_inputs
        .mean_reversion_fitted_parameters
        .kappa;

    for interval_index in 0..run_request.horizon_interval_count {
        let interval_start_utc_nanos = run_request
            .start_time_utc_nanos
            .checked_add((interval_index as i64).saturating_mul(interval_delta_nanos))
            .ok_or(CoarseSimulationRunError::IntervalBoundaryOverflow { interval_index })?;

        let interval_end_utc_nanos = interval_start_utc_nanos
            .checked_add(interval_delta_nanos)
            .ok_or(CoarseSimulationRunError::IntervalBoundaryOverflow { interval_index })?;

        let interval_settings = adaptive_transform_slice_settings(
            kernel_parameters.lambda_bar,
            run_request.interval_factors.s_h[interval_index],
            run_request.interval_delta,
        );

        let current_statistics_context = SufficientStatisticsTransformSliceContext::try_new(
            interval_index,
            run_request.interval_factors.s_h[interval_index],
            endogenous_statistics_news_intensity_input(),
            run_request.interval_delta,
            terminal_state,
        )
        .map_err(
            |source| CoarseSimulationRunError::InvalidTransformSliceContext {
                interval_index,
                source,
            },
        )?;

        let solved_transform_slices = solve_sufficient_statistics_transform_slices(
            model_inputs.risk_model_inputs.equation_specification,
            current_statistics_context,
            interval_settings,
        )
        .map_err(
            |source| CoarseSimulationRunError::TransformSliceSolveFailure {
                interval_index,
                source,
            },
        )?;

        let interval_statistics_seed =
            janiform_foundations::randomness::derive_simulation_substream_seed(
                run_request.seed,
                SimulationRandomnessSubstreamDomain::CoarseStatisticsSampling,
                interval_index as u64,
            );

        let sampled_statistics =
            sample_interval_statistics_from_transform_slices_with_randomness_factory(
                interval_statistics_seed,
                solved_transform_slices,
                randomness_factory,
            )
            .map_err(|source| {
                CoarseSimulationRunError::TransformSliceSamplingFailure {
                    interval_index,
                    source,
                }
            })?;

        let known_news_timestamps = &model_inputs
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
            next_known_news_timestamp_position,
            interval_start_utc_nanos,
            interval_end_utc_nanos,
        );

        next_known_news_timestamp_position = next_interval_known_news_timestamp_position;

        let interval_known_news_timestamp_slice = &known_news_timestamps
            [interval_known_news_start_position..interval_known_news_end_position];

        validate_known_news_factor_alignment(
            interval_index,
            run_request.interval_delta,
            run_request.interval_factors.nu_h_known[interval_index],
            interval_known_news_timestamp_slice.len(),
        )?;

        let interval_direct_jump_totals = sample_interval_direct_jump_totals(
            &run_request,
            &calendar_registry,
            interval_index,
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            interval_known_news_timestamp_slice,
            randomness_factory,
        )?;

        let mut interval_return_stream = randomness_factory.create_substream(
            run_request.seed,
            SimulationRandomnessSubstreamDomain::CoarseTradingTimeReturn,
            interval_index as u64,
        );

        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: sampled_statistics.a_plus,
            a_minus: sampled_statistics.a_minus,
            open_jump: interval_direct_jump_totals.open_jump_return,
            closed_gap: interval_direct_jump_totals.closed_gap_return,
        })
        .map_err(
            |source| CoarseSimulationRunError::InvalidReturnDecomposition {
                interval_index,
                source,
            },
        )?;

        let trading_time_contributions = return_decomposition
            .sample_nig_trading_time_increment(driver_parameters, &mut interval_return_stream)
            .map_err(
                |source| CoarseSimulationRunError::TradingTimeSamplingFailure {
                    interval_index,
                    source,
                },
            )?;

        let buy_side_return = trading_time_contributions.buy_side_increment;
        let sell_side_return = -trading_time_contributions.sell_side_increment;
        let trading_time_return = trading_time_contributions.combined_increment;
        let total_return = trading_time_return
            + interval_direct_jump_totals.open_jump_return
            + interval_direct_jump_totals.closed_gap_return;

        let return_components = SufficientStatisticsRiskReturnComponents::try_new(
            buy_side_return,
            sell_side_return,
            trading_time_return,
            interval_direct_jump_totals.open_jump_return,
            interval_direct_jump_totals.closed_gap_return,
            total_return,
        )
        .map_err(|source| CoarseSimulationRunError::InvalidReturnComponents {
            interval_index,
            source,
        })?;

        let updated_observed_boundary_state =
            evolve_mean_reversion_boundary_state_on_interval_grid(
                observed_boundary_state.clone(),
                mean_reversion_kappa,
                run_request.interval_delta,
                total_return,
                model_inputs
                    .risk_model_inputs
                    .mean_reversion_runtime_interval_anchors
                    .end_anchor_values[interval_index],
            )
            .map_err(|source| {
                CoarseSimulationRunError::MeanReversionBoundaryStateUpdateFailure {
                    interval_index,
                    source,
                }
            })?;

        let observed_interval_return = updated_observed_boundary_state.observed_log_price
            - observed_boundary_state.observed_log_price;

        let path_record = CoarseSimulationPathRecord::try_new(
            interval_index,
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            sampled_statistics,
            return_components,
            total_return,
            observed_interval_return,
            updated_observed_boundary_state.observed_log_price,
            updated_observed_boundary_state.anchor_value,
            updated_observed_boundary_state.deviation,
        )
        .map_err(|source| CoarseSimulationRunError::InvalidPathRecord {
            interval_index,
            source,
        })?;

        records.push(path_record);
        observed_boundary_state = updated_observed_boundary_state;

        let boundary_update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
            interval_index,
            run_request.interval_delta,
            terminal_state,
            sampled_statistics,
        );

        let state_after_count_update = update_boundary_state_from_interval_statistics(
            kernel_parameters,
            boundary_update_snapshot,
        )
        .map_err(
            |source| CoarseSimulationRunError::BoundaryStateUpdateFailure {
                interval_index,
                source,
            },
        )?;

        terminal_state = apply_interval_news_activity_coupling_at_boundary(
            state_after_count_update,
            kernel_parameters,
            interval_index,
            interval_direct_jump_totals.activity_coupling_j_t_increment,
        )?;
    }

    CoarseSimulationPath::try_new(
        run_request.seed,
        run_request.start_time_utc_nanos,
        run_request.interval_delta,
        run_request.horizon_interval_count,
        model_inputs.risk_model_inputs.initial_excitation_state,
        terminal_state,
        records,
    )
    .map_err(|source| CoarseSimulationRunError::InvalidPathArtifact { source })
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationRunError {
    InvalidRunRequest {
        source: CoarseSimulationRunRequestError,
    },
    CalendarRegistryLoadFailure {
        source: crate::calendar::ExchangeCalendarRegistryLoadError,
    },
    InvalidNigDriverConfiguration {
        source: janiform_model_kernels::nig::NigSharedPlusShiftConfigurationError,
    },
    InvalidIntervalDelta {
        source: super::coarse::IntervalDeltaGridError,
    },
    IntervalBoundaryOverflow {
        interval_index: usize,
    },
    InvalidTransformSliceContext {
        interval_index: usize,
        source: crate::risk::SufficientStatisticsTransformSliceContextError,
    },
    TransformSliceSolveFailure {
        interval_index: usize,
        source: SufficientStatisticsTransformSliceSolveError,
    },
    TransformSliceSamplingFailure {
        interval_index: usize,
        source: SufficientStatisticsTransformSliceSamplingError,
    },
    KnownNewsFactorAlignmentFailure {
        interval_index: usize,
        expected_known_nu_h: f64,
        derived_known_nu_h: f64,
    },
    NewsProcessConfigurationFailure {
        interval_index: usize,
        source: NewsProcessConfigurationError,
    },
    NewsMarkSamplingFailure {
        interval_index: usize,
        source: NewsMarkInterfaceError,
    },
    SharedNewsDispatchFailure {
        interval_index: usize,
        source: SharedNewsEventDispatchError,
    },
    InvalidReturnDecomposition {
        interval_index: usize,
        source: janiform_model_kernels::return_decomposition::ReturnDecompositionValidationError,
    },
    TradingTimeSamplingFailure {
        interval_index: usize,
        source: janiform_model_kernels::nig::NigTradingTimeIncrementSamplingError,
    },
    InvalidReturnComponents {
        interval_index: usize,
        source: SufficientStatisticsRiskReturnComponentsError,
    },
    InvalidPathRecord {
        interval_index: usize,
        source: super::coarse::CoarseSimulationPathRecordError,
    },
    MeanReversionBoundaryStateUpdateFailure {
        interval_index: usize,
        source: janiform_model_kernels::mean_reversion::MeanReversionBoundaryStateEvolutionError,
    },
    BoundaryStateUpdateFailure {
        interval_index: usize,
        source: SufficientStatisticsBoundaryStateUpdateError,
    },
    NewsBoundaryStateUpdateFailure {
        interval_index: usize,
        source: HawkesTransitionError,
    },
    InvalidPathArtifact {
        source: CoarseSimulationPathError,
    },
}

impl Display for CoarseSimulationRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRunRequest { source } => {
                write!(formatter, "run_request is invalid: {source}")
            }
            Self::CalendarRegistryLoadFailure { source } => {
                write!(
                    formatter,
                    "failed to load calendar registry from run_request.model_inputs: {source}"
                )
            }
            Self::InvalidNigDriverConfiguration { source } => {
                write!(
                    formatter,
                    "nig_shared_plus_shift_configuration is invalid: {source}"
                )
            }
            Self::InvalidIntervalDelta { source } => {
                write!(formatter, "interval_delta is invalid: {source}")
            }
            Self::IntervalBoundaryOverflow { interval_index } => {
                write!(
                    formatter,
                    "interval {interval_index} boundary timestamps overflowed i64"
                )
            }
            Self::InvalidTransformSliceContext {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} transform-slice context is invalid: {source}"
            ),
            Self::TransformSliceSolveFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} transform-slice solve failed: {source}"
            ),
            Self::TransformSliceSamplingFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} sufficient-statistics sampling failed: {source}"
            ),
            Self::KnownNewsFactorAlignmentFailure {
                interval_index,
                expected_known_nu_h,
                derived_known_nu_h,
            } => write!(
                formatter,
                "interval {interval_index} known-news factor mismatch, expected nu_h_known={expected_known_nu_h} from interval_factors and derived {derived_known_nu_h} from scheduled news timestamps"
            ),
            Self::NewsProcessConfigurationFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} news_process_configuration is invalid: {source}"
            ),
            Self::NewsMarkSamplingFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} shared news mark sampling failed: {source}"
            ),
            Self::SharedNewsDispatchFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} shared news dispatch failed: {source}"
            ),
            Self::InvalidReturnDecomposition {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} return decomposition is invalid: {source}"
            ),
            Self::TradingTimeSamplingFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} trading-time increment sampling failed: {source}"
            ),
            Self::InvalidReturnComponents {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} return components are invalid: {source}"
            ),
            Self::InvalidPathRecord {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} path record is invalid: {source}"
            ),
            Self::MeanReversionBoundaryStateUpdateFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} mean-reversion observed-state update failed: {source}"
            ),
            Self::BoundaryStateUpdateFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} boundary count-mapped state update failed: {source}"
            ),
            Self::NewsBoundaryStateUpdateFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} news activity-coupling boundary update failed: {source}"
            ),
            Self::InvalidPathArtifact { source } => {
                write!(
                    formatter,
                    "coarse simulation path artifact is invalid: {source}"
                )
            }
        }
    }
}

impl Error for CoarseSimulationRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRunRequest { source } => Some(source),
            Self::CalendarRegistryLoadFailure { source } => Some(source),
            Self::InvalidNigDriverConfiguration { source } => Some(source),
            Self::InvalidIntervalDelta { source } => Some(source),
            Self::InvalidTransformSliceContext { source, .. } => Some(source),
            Self::TransformSliceSolveFailure { source, .. } => Some(source),
            Self::TransformSliceSamplingFailure { source, .. } => Some(source),
            Self::NewsProcessConfigurationFailure { source, .. } => Some(source),
            Self::NewsMarkSamplingFailure { source, .. } => Some(source),
            Self::SharedNewsDispatchFailure { source, .. } => Some(source),
            Self::InvalidReturnDecomposition { source, .. } => Some(source),
            Self::TradingTimeSamplingFailure { source, .. } => Some(source),
            Self::InvalidReturnComponents { source, .. } => Some(source),
            Self::InvalidPathRecord { source, .. } => Some(source),
            Self::MeanReversionBoundaryStateUpdateFailure { source, .. } => Some(source),
            Self::BoundaryStateUpdateFailure { source, .. } => Some(source),
            Self::NewsBoundaryStateUpdateFailure { source, .. } => Some(source),
            Self::InvalidPathArtifact { source } => Some(source),
            Self::IntervalBoundaryOverflow { .. }
            | Self::KnownNewsFactorAlignmentFailure { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct IntervalDirectJumpTotals {
    activity_coupling_j_t_increment: f64,
    open_jump_return: f64,
    closed_gap_return: f64,
}

fn sample_interval_direct_jump_totals<F>(
    run_request: &CoarseSimulationRunRequest,
    calendar_registry: &crate::calendar::ExchangeCalendarRegistry,
    interval_index: usize,
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
    interval_known_news_timestamp_slice: &[i64],
    randomness_factory: &F,
) -> Result<IntervalDirectJumpTotals, CoarseSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let model_inputs = &run_request.model_inputs;
    model_inputs
        .news_process_configuration
        .validate()
        .map_err(
            |source| CoarseSimulationRunError::NewsProcessConfigurationFailure {
                interval_index,
                source,
            },
        )?;

    let mut news_count_stream = randomness_factory.create_substream(
        run_request.seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsCount,
        interval_index as u64,
    );
    let mut news_timestamp_stream = randomness_factory.create_substream(
        run_request.seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp,
        interval_index as u64,
    );
    let mut news_mark_stream = randomness_factory.create_substream(
        run_request.seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsMarks,
        interval_index as u64,
    );

    let mut event_inputs: Vec<SharedNewsEventTimestampedInput> =
        Vec::with_capacity(interval_known_news_timestamp_slice.len());

    let mut next_generation_order = 0_u64;

    let mark_pipeline = model_inputs
        .news_process_configuration
        .default_mark_pipeline();

    for &known_news_timestamp_utc_nanos in interval_known_news_timestamp_slice {
        event_inputs.push(
            build_timestamped_news_event_input(
                &mark_pipeline,
                NewsArrivalComponent::Known,
                known_news_timestamp_utc_nanos,
                next_generation_order,
                model_inputs
                    .news_process_configuration
                    .known_component_mark_scales,
                &mut news_mark_stream,
                interval_index,
            )
            .map_err(|source| CoarseSimulationRunError::NewsMarkSamplingFailure {
                interval_index,
                source,
            })?,
        );

        next_generation_order = next_generation_order.saturating_add(1);
    }

    let interval_surprise_expected_count =
        run_request.interval_factors.nu_h_surprise[interval_index] * run_request.interval_delta;
    let interval_surprise_news_count =
        sample_count_from_expectation(interval_surprise_expected_count, &mut news_count_stream);

    let interval_duration_nanos = interval_end_utc_nanos - interval_start_utc_nanos;

    for _ in 0..interval_surprise_news_count {
        let timestamp_utc_nanos = sample_interval_timestamp_utc_nanos(
            interval_start_utc_nanos,
            interval_duration_nanos,
            &mut news_timestamp_stream,
        );

        event_inputs.push(
            build_timestamped_news_event_input(
                &mark_pipeline,
                NewsArrivalComponent::Surprise,
                timestamp_utc_nanos,
                next_generation_order,
                model_inputs
                    .news_process_configuration
                    .surprise_component_mark_scales,
                &mut news_mark_stream,
                interval_index,
            )
            .map_err(|source| CoarseSimulationRunError::NewsMarkSamplingFailure {
                interval_index,
                source,
            })?,
        );

        next_generation_order = next_generation_order.saturating_add(1);
    }

    let dispatch_output = dispatch_shared_news_events_with_resolved_market_states(
        calendar_registry,
        &model_inputs.exchange_code,
        SharedNewsAccumulatorState::new(0.0, 0.0, 0.0),
        event_inputs.as_slice(),
    )
    .map_err(
        |source| CoarseSimulationRunError::SharedNewsDispatchFailure {
            interval_index,
            source,
        },
    )?;

    Ok(IntervalDirectJumpTotals {
        activity_coupling_j_t_increment: dispatch_output.final_state.j_t,
        open_jump_return: dispatch_output.final_state.m_t,
        closed_gap_return: dispatch_output.final_state.g_t,
    })
}

fn apply_interval_news_activity_coupling_at_boundary(
    state_before_news_update: janiform_model_kernels::hawkes::HawkesExcitationState,
    kernel_parameters: janiform_model_kernels::hawkes::HawkesKernelParameters,
    interval_index: usize,
    activity_coupling_j_t_increment: f64,
) -> Result<janiform_model_kernels::hawkes::HawkesExcitationState, CoarseSimulationRunError> {
    if activity_coupling_j_t_increment == 0.0 {
        return Ok(state_before_news_update);
    }

    // The coarse boundary state already uses count-mapped endogenous aggregation.
    // Apply the interval-total exogenous news loading as one additional zero-time jump.
    evolve_hawkes_state(
        state_before_news_update,
        kernel_parameters,
        HawkesTransitionStep::new(0.0, None, Some(activity_coupling_j_t_increment)),
    )
    .map_err(
        |source| CoarseSimulationRunError::NewsBoundaryStateUpdateFailure {
            interval_index,
            source,
        },
    )
}

fn validate_known_news_factor_alignment(
    interval_index: usize,
    interval_delta: f64,
    expected_known_nu_h: f64,
    known_news_event_count: usize,
) -> Result<(), CoarseSimulationRunError> {
    let derived_known_nu_h = known_news_event_count as f64 / interval_delta;
    if (expected_known_nu_h - derived_known_nu_h).abs() > KNOWN_NEWS_INTENSITY_ALIGNMENT_TOLERANCE {
        return Err(CoarseSimulationRunError::KnownNewsFactorAlignmentFailure {
            interval_index,
            expected_known_nu_h,
            derived_known_nu_h,
        });
    }

    Ok(())
}

fn adaptive_transform_slice_settings(
    lambda_bar: f64,
    seasonality_factor: f64,
    interval_delta: f64,
) -> SufficientStatisticsTransformSliceSettings {
    let expected_side_scale = (lambda_bar * seasonality_factor.max(0.0) * interval_delta).max(1.0);
    let probe_distance = (TARGET_TRANSFORM_PROBE_SHIFT / expected_side_scale).clamp(
        MIN_TRANSFORM_PROBE_DISTANCE,
        SufficientStatisticsTransformSliceSettings::default().a_plus_u,
    );

    SufficientStatisticsTransformSliceSettings {
        a_plus_u: probe_distance,
        a_minus_u: probe_distance,
        n_plus_z: 1.0 - probe_distance,
        n_minus_z: 1.0 - probe_distance,
    }
}

fn endogenous_statistics_news_intensity_input() -> ExponentialAffineTransformNewsIntensityInput {
    // Coarse runtime samples exogenous news explicitly for direct jumps and boundary excitation.
    // Keep the transform-slice statistics endogenous-only so the same news process is not
    // absorbed once in the affine counts and again in the explicit event sampler.
    ExponentialAffineTransformNewsIntensityInput::try_new(0.0, 0.0)
        .expect("zero news intensity must be a valid affine input")
}

#[cfg(test)]
mod tests {
    use super::{adaptive_transform_slice_settings, endogenous_statistics_news_intensity_input};

    #[test]
    fn adaptive_transform_slice_settings_stay_linear_for_high_intensity_regimes() {
        let expected_side_scale = 2_500.0;
        let settings = adaptive_transform_slice_settings(2_500.0, 1.0, 1.0);

        let recovered_integrated_intensity =
            (1.0 - (-settings.a_plus_u * expected_side_scale).exp()) / settings.a_plus_u;
        let recovered_count = (((settings.n_plus_z - 1.0) * expected_side_scale).exp() - 1.0)
            / (settings.n_plus_z - 1.0);

        let integrated_intensity_relative_error =
            (recovered_integrated_intensity - expected_side_scale).abs() / expected_side_scale;
        let count_relative_error =
            (recovered_count - expected_side_scale).abs() / expected_side_scale;

        assert!(
            integrated_intensity_relative_error < 0.02,
            "adaptive A probe should stay near the linear regime, relative_error={integrated_intensity_relative_error}"
        );
        assert!(
            count_relative_error < 0.02,
            "adaptive count probe should stay near the linear regime, relative_error={count_relative_error}"
        );
    }

    #[test]
    fn endogenous_statistics_news_intensity_input_is_zeroed() {
        let input = endogenous_statistics_news_intensity_input();

        assert_eq!(input.known_nu_h, 0.0);
        assert_eq!(input.surprise_nu_h, 0.0);
    }
}
