use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_affine_oracle::endogenous_anchor::{
    EndogenousAnchorIntervalFactors, derive_expected_interval_projection_from_first_moment_dynamics,
};
use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsReturnComponents, SufficientStatisticsReturnComponentsError,
    SufficientStatisticsTransformSliceSamplingError,
    sample_interval_statistics_from_expected_statistics_with_randomness_factory,
};
use janiform_foundations::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessSubstreamDomain,
    SplitMix64SimulationRandomnessFactory,
};
use janiform_model_kernels::hawkes::{
    HawkesTransitionError, HawkesTransitionStep, evolve_hawkes_state,
};
use janiform_model_kernels::news::{
    NewsArrivalComponent, NewsMarkInterfaceError, NewsProcessConfigurationError,
    SharedNewsAccumulatorState, SharedNewsEventDispatchError, SharedNewsEventTimestampedInput,
    dispatch_shared_news_events_with_resolved_market_states,
};
use janiform_model_kernels::return_clock::{
    EffectiveReturnClocksBuildError, IntegratedSideActivityDecomposition,
    compute_effective_return_clocks,
};
use janiform_model_kernels::return_decomposition::{ReturnDecomposition, ReturnDecompositionInput};

use super::coarse::{CoarseSimulationPath, CoarseSimulationPathError, CoarseSimulationPathRecord};
use super::long_variance::{
    LongVarianceTransitionSamplingError, apply_long_variance_event_count_jumps,
    sample_long_variance_transition,
};
use super::progress::{SimulationProgressEvent, SimulationProgressPhase, emit_progress_event};
use super::runtime::{
    SeededObservedState, SeededSimulationMarketStateResolver, build_interval_open_market_segments,
    build_timestamped_news_event_input, derive_known_news_interval_slice_bounds,
    integrate_open_market_elapsed_hours, sample_count_from_expectation,
    sample_interval_timestamp_utc_nanos,
};
use super::{CoarseSimulationRunRequest, CoarseSimulationRunRequestError};

const KNOWN_NEWS_INTENSITY_ALIGNMENT_TOLERANCE: f64 = 1.0e-12;

/// Run deterministic coarse-interval simulation using the default splitmix64 randomness policy.
pub fn run_coarse_simulation(
    run_request: CoarseSimulationRunRequest,
) -> Result<CoarseSimulationPath, CoarseSimulationRunError> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;
    run_coarse_simulation_with_randomness_factory(run_request, &randomness_factory)
}

pub fn run_coarse_simulation_with_observer(
    run_request: CoarseSimulationRunRequest,
    observer: &mut dyn FnMut(SimulationProgressEvent),
) -> Result<CoarseSimulationPath, CoarseSimulationRunError> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;

    run_coarse_simulation_with_randomness_factory_and_observer(
        run_request,
        &randomness_factory,
        observer,
    )
}

/// Run deterministic coarse-interval simulation with an injected randomness factory.
pub fn run_coarse_simulation_with_randomness_factory<F>(
    run_request: CoarseSimulationRunRequest,
    randomness_factory: &F,
) -> Result<CoarseSimulationPath, CoarseSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    run_coarse_simulation_with_randomness_factory_internal(run_request, randomness_factory, None)
}

pub fn run_coarse_simulation_with_randomness_factory_and_observer<F>(
    run_request: CoarseSimulationRunRequest,
    randomness_factory: &F,
    observer: &mut dyn FnMut(SimulationProgressEvent),
) -> Result<CoarseSimulationPath, CoarseSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    run_coarse_simulation_with_randomness_factory_internal(
        run_request,
        randomness_factory,
        Some(observer),
    )
}

fn run_coarse_simulation_with_randomness_factory_internal<F>(
    run_request: CoarseSimulationRunRequest,
    randomness_factory: &F,
    mut observer: Option<&mut dyn FnMut(SimulationProgressEvent)>,
) -> Result<CoarseSimulationPath, CoarseSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    run_request
        .validate()
        .map_err(|source| CoarseSimulationRunError::InvalidRunRequest {
            source: Box::new(source),
        })?;

    let model_inputs = &run_request.model_inputs;

    let calendar_registry = model_inputs.load_calendar_registry().map_err(|source| {
        CoarseSimulationRunError::CalendarRegistryLoadFailure {
            source: Box::new(source),
        }
    })?;

    let kernel_parameters = model_inputs
        .sufficient_statistics_model_inputs
        .equation_specification
        .hawkes_kernel_parameters;

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

    let long_variance_stationary_mean_reversion_level = model_inputs
        .long_variance_parameters
        .stationary_mean_reversion_level_for_hawkes_kernel(kernel_parameters)
        .map_err(
            |source| CoarseSimulationRunError::InvalidLongVarianceStationaryLevel {
                source: Box::new(source),
            },
        )?;

    let interval_delta_nanos = run_request.interval_delta_nanos().map_err(|source| {
        CoarseSimulationRunError::InvalidIntervalDelta {
            source: Box::new(source),
        }
    })?;

    let mut terminal_state = model_inputs
        .sufficient_statistics_model_inputs
        .initial_excitation_state;

    let mut long_variance_state = model_inputs.initial_long_variance_state;

    let mut observed_state = SeededObservedState::from_boundary_state(
        &model_inputs
            .sufficient_statistics_model_inputs
            .initial_mean_reversion_boundary_state,
    );

    let mut next_known_news_timestamp_position = 0usize;

    let mut records: Vec<CoarseSimulationPathRecord> =
        Vec::with_capacity(run_request.horizon_interval_count);

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

    let interval_start_utc_nanos = (0..run_request.horizon_interval_count)
        .map(|interval_index| {
            run_request
                .start_time_utc_nanos
                .checked_add((interval_index as i64).saturating_mul(interval_delta_nanos))
                .ok_or(CoarseSimulationRunError::IntervalBoundaryOverflow { interval_index })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let interval_end_utc_nanos = interval_start_utc_nanos
        .iter()
        .copied()
        .enumerate()
        .map(|(interval_index, interval_start_utc_nanos)| {
            interval_start_utc_nanos
                .checked_add(interval_delta_nanos)
                .ok_or(CoarseSimulationRunError::IntervalBoundaryOverflow { interval_index })
        })
        .collect::<Result<Vec<_>, _>>()?;

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

    emit_progress_event(
        &mut observer,
        SimulationProgressEvent::new(
            SimulationProgressPhase::CoarseIntervals,
            0,
            run_request.horizon_interval_count,
            interval_delta_nanos,
            None,
        ),
    );

    for interval_index in 0..run_request.horizon_interval_count {
        let interval_start_utc_nanos = interval_start_utc_nanos[interval_index];
        let interval_end_utc_nanos = interval_end_utc_nanos[interval_index];

        let endogenous_interval_factors = EndogenousAnchorIntervalFactors::try_new(
            run_request.interval_factors.s_h[interval_index],
            0.0,
            0.0,
            run_request.interval_delta,
        )
        .map_err(|source| {
            CoarseSimulationRunError::EndogenousFirstMomentProjectionFailure {
                interval_index,
                message: source.to_string(),
            }
        })?;

        let first_moment_projection =
            derive_expected_interval_projection_from_first_moment_dynamics(
                interval_index,
                model_inputs
                    .sufficient_statistics_model_inputs
                    .equation_specification,
                expected_news_severity_mark,
                endogenous_interval_factors,
                terminal_state,
            )
            .map_err(|source| {
                CoarseSimulationRunError::EndogenousFirstMomentProjectionFailure {
                    interval_index,
                    message: source.to_string(),
                }
            })?;

        let interval_statistics_seed =
            janiform_foundations::randomness::derive_simulation_substream_seed(
                run_request.seed,
                SimulationRandomnessSubstreamDomain::CoarseStatisticsSampling,
                interval_index as u64,
            );

        let sampled_statistics =
            sample_interval_statistics_from_expected_statistics_with_randomness_factory(
                interval_statistics_seed,
                interval_index,
                first_moment_projection.expected_statistics,
                randomness_factory,
            )
            .map_err(|source| {
                CoarseSimulationRunError::TransformSliceSamplingFailure {
                    interval_index,
                    source: Box::new(source),
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

        let mut long_variance_stream = randomness_factory.create_substream(
            run_request.seed,
            SimulationRandomnessSubstreamDomain::LongVarianceDiffusion,
            interval_index as u64,
        );

        let long_variance_transition = sample_long_variance_transition(
            long_variance_state,
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
            open_jump: interval_direct_jump_totals.open_jump_return,
            closed_gap: interval_direct_jump_totals.closed_gap_return,
        })
        .map_err(
            |source| CoarseSimulationRunError::InvalidReturnDecomposition {
                interval_index,
                source: Box::new(source),
            },
        )?;

        let trading_time_contributions = return_decomposition
            .sample_nig_trading_time_increment(driver_parameters, &mut interval_return_stream)
            .map_err(
                |source| CoarseSimulationRunError::TradingTimeSamplingFailure {
                    interval_index,
                    source: Box::new(source),
                },
            )?;

        let buy_side_return = trading_time_contributions.buy_side_increment;
        let sell_side_return = -trading_time_contributions.sell_side_increment;

        let trading_time_return = trading_time_contributions.combined_increment;

        let raw_total_return = trading_time_return
            + interval_direct_jump_totals.open_jump_return
            + interval_direct_jump_totals.closed_gap_return;

        let previous_observed_log_price = observed_state.observed_log_price;

        let elapsed_open_hours = integrate_open_market_elapsed_hours(
            interval_open_market_segments[interval_index].as_slice(),
            interval_start_utc_nanos,
            interval_end_utc_nanos,
        );

        let projected_core_increment = observed_state
            .project_core_increment(
                mean_reversion_kappa,
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
            interval_direct_jump_totals.open_jump_return,
            interval_direct_jump_totals.closed_gap_return,
            total_return,
        )
        .map_err(|source| CoarseSimulationRunError::InvalidReturnComponents {
            interval_index,
            source: Box::new(source),
        })?;

        let updated_observed_state = observed_state
            .apply_projected_core_increment(
                mean_reversion_kappa,
                elapsed_open_hours,
                projected_core_increment,
            )
            .apply_anchor_reset(
                model_inputs
                    .sufficient_statistics_model_inputs
                    .mean_reversion_runtime_interval_anchors
                    .end_anchor_values[interval_index],
                model_inputs
                    .sufficient_statistics_model_inputs
                    .mean_reversion_runtime_interval_anchors
                    .end_pressure_states[interval_index],
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
            updated_observed_state.pressure_state,
            updated_observed_state.deviation_from_anchor,
        )
        .map_err(|source| CoarseSimulationRunError::InvalidPathRecord {
            interval_index,
            source: Box::new(source),
        })?;

        records.push(path_record);

        observed_state = updated_observed_state;

        terminal_state = apply_interval_news_activity_coupling_at_boundary(
            first_moment_projection.structural_state_after,
            kernel_parameters,
            interval_index,
            interval_direct_jump_totals.activity_coupling_j_t_increment,
        )?;

        long_variance_state = apply_long_variance_event_count_jumps(
            long_variance_transition.terminal_state,
            model_inputs.long_variance_parameters,
            sampled_statistics.n_plus,
            sampled_statistics.n_minus,
        )
        .map_err(
            |source| CoarseSimulationRunError::LongVarianceTransitionFailure {
                interval_index,
                source: Box::new(source),
            },
        )?;

        emit_progress_event(
            &mut observer,
            SimulationProgressEvent::new(
                SimulationProgressPhase::CoarseIntervals,
                interval_index + 1,
                run_request.horizon_interval_count,
                interval_delta_nanos,
                None,
            ),
        );
    }

    CoarseSimulationPath::try_new(
        run_request.seed,
        run_request.start_time_utc_nanos,
        run_request.interval_delta,
        run_request.horizon_interval_count,
        model_inputs
            .sufficient_statistics_model_inputs
            .initial_excitation_state,
        terminal_state,
        records,
    )
    .map_err(|source| CoarseSimulationRunError::InvalidPathArtifact {
        source: Box::new(source),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationRunError {
    InvalidRunRequest {
        source: Box<CoarseSimulationRunRequestError>,
    },
    CalendarRegistryLoadFailure {
        source: Box<janiform_market_time::calendar::ExchangeCalendarRegistryLoadError>,
    },
    InvalidNigDriverConfiguration {
        source: Box<janiform_model_kernels::nig::NigSharedPlusShiftConfigurationError>,
    },
    InvalidLongVarianceStationaryLevel {
        source: Box<
            janiform_model_kernels::long_variance::LongVarianceStationaryLevelForHawkesKernelError,
        >,
    },
    InvalidIntervalDelta {
        source: Box<super::coarse::IntervalDeltaGridError>,
    },
    IntervalBoundaryOverflow {
        interval_index: usize,
    },
    EndogenousFirstMomentProjectionFailure {
        interval_index: usize,
        message: String,
    },
    TransformSliceSamplingFailure {
        interval_index: usize,
        source: Box<SufficientStatisticsTransformSliceSamplingError>,
    },
    KnownNewsFactorAlignmentFailure {
        interval_index: usize,
        expected_known_nu_h: f64,
        derived_known_nu_h: f64,
    },
    NewsProcessConfigurationFailure {
        interval_index: usize,
        source: Box<NewsProcessConfigurationError>,
    },
    NewsMarkSamplingFailure {
        interval_index: usize,
        source: Box<NewsMarkInterfaceError>,
    },
    SharedNewsDispatchFailure {
        interval_index: usize,
        source: Box<SharedNewsEventDispatchError>,
    },
    InvalidReturnDecomposition {
        interval_index: usize,
        source:
            Box<janiform_model_kernels::return_decomposition::ReturnDecompositionValidationError>,
    },
    TradingTimeSamplingFailure {
        interval_index: usize,
        source: Box<janiform_model_kernels::nig::NigTradingTimeIncrementSamplingError>,
    },
    EffectiveReturnClockBuildFailure {
        interval_index: usize,
        source: Box<EffectiveReturnClocksBuildError>,
    },
    LongVarianceTransitionFailure {
        interval_index: usize,
        source: Box<LongVarianceTransitionSamplingError>,
    },
    TickGridProjectionFailure {
        interval_index: usize,
        source: Box<super::error::SeededTickSimulationRunError>,
    },
    InvalidReturnComponents {
        interval_index: usize,
        source: Box<SufficientStatisticsReturnComponentsError>,
    },
    InvalidPathRecord {
        interval_index: usize,
        source: Box<super::coarse::CoarseSimulationPathRecordError>,
    },
    OpenMarketExposureResolutionFailure {
        source: Box<super::error::SeededTickSimulationRunError>,
    },
    NewsBoundaryStateUpdateFailure {
        interval_index: usize,
        source: Box<HawkesTransitionError>,
    },
    InvalidPathArtifact {
        source: Box<CoarseSimulationPathError>,
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
            Self::InvalidLongVarianceStationaryLevel { source } => write!(
                formatter,
                "long_variance_parameters imply an invalid stationary mean-reversion level: {source}"
            ),
            Self::InvalidIntervalDelta { source } => {
                write!(formatter, "interval_delta is invalid: {source}")
            }
            Self::IntervalBoundaryOverflow { interval_index } => {
                write!(
                    formatter,
                    "interval {interval_index} boundary timestamps overflowed i64"
                )
            }
            Self::EndogenousFirstMomentProjectionFailure {
                interval_index,
                message,
            } => write!(
                formatter,
                "interval {interval_index} endogenous first-moment projection failed: {message}"
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
            Self::EffectiveReturnClockBuildFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} effective return-clock construction failed: {source}"
            ),
            Self::LongVarianceTransitionFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} long-variance transition failed: {source}"
            ),
            Self::TickGridProjectionFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} tick-grid observation projection failed: {source}"
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
            Self::OpenMarketExposureResolutionFailure { source } => write!(
                formatter,
                "failed to resolve coarse open-market exposure: {source}"
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
            Self::InvalidRunRequest { source } => Some(source.as_ref()),
            Self::CalendarRegistryLoadFailure { source } => Some(source.as_ref()),
            Self::InvalidNigDriverConfiguration { source } => Some(source.as_ref()),
            Self::InvalidLongVarianceStationaryLevel { source } => Some(source.as_ref()),
            Self::InvalidIntervalDelta { source } => Some(source.as_ref()),
            Self::TransformSliceSamplingFailure { source, .. } => Some(source.as_ref()),
            Self::NewsProcessConfigurationFailure { source, .. } => Some(source.as_ref()),
            Self::NewsMarkSamplingFailure { source, .. } => Some(source.as_ref()),
            Self::SharedNewsDispatchFailure { source, .. } => Some(source.as_ref()),
            Self::InvalidReturnDecomposition { source, .. } => Some(source.as_ref()),
            Self::TradingTimeSamplingFailure { source, .. } => Some(source.as_ref()),
            Self::EffectiveReturnClockBuildFailure { source, .. } => Some(source.as_ref()),
            Self::LongVarianceTransitionFailure { source, .. } => Some(source.as_ref()),
            Self::TickGridProjectionFailure { source, .. } => Some(source.as_ref()),
            Self::InvalidReturnComponents { source, .. } => Some(source.as_ref()),
            Self::InvalidPathRecord { source, .. } => Some(source.as_ref()),
            Self::OpenMarketExposureResolutionFailure { source } => Some(source.as_ref()),
            Self::NewsBoundaryStateUpdateFailure { source, .. } => Some(source.as_ref()),
            Self::InvalidPathArtifact { source } => Some(source.as_ref()),
            Self::IntervalBoundaryOverflow { .. }
            | Self::EndogenousFirstMomentProjectionFailure { .. }
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
    calendar_registry: &janiform_market_time::calendar::ExchangeCalendarRegistry,
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
                source: Box::new(source),
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
                source: Box::new(source),
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
                source: Box::new(source),
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
            source: Box::new(source),
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
            source: Box::new(source),
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

#[cfg(test)]
mod tests {
    use super::run_coarse_simulation;
    use crate::coarse::{CoarseSimulationModelInputs, CoarseSimulationRunRequest};
    use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
    use janiform_affine_oracle::sufficient_statistics::{
        SufficientStatisticsIntervalFactors, SufficientStatisticsModelInputs,
    };
    use janiform_market_time::calendar::{
        CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarRegistryArtifact,
        RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
    };
    use janiform_model_kernels::hawkes::{
        HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
    };
    use janiform_model_kernels::long_variance::{LongVarianceParameters, LongVarianceState};
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
                long_variance_parameters: LongVarianceParameters::neutral(),
                initial_long_variance_state: LongVarianceState::unit(),
                mean_reversion_fitted_parameters:
                    MeanReversionFittedParameters::flat_anchor_mean_reversion(1.0)
                        .expect("positive-kappa mean reversion should construct"),
                mean_reversion_runtime_interval_anchors:
                    MeanReversionRuntimeIntervalAnchors::try_new(
                        "explicit_runtime_interval_grid_alignment.v1",
                        2.0,
                        0.0,
                        vec![2.0],
                        vec![0.0],
                        vec![1.0],
                    )
                    .expect("mean-reversion runtime anchors should construct"),
                initial_mean_reversion_boundary_state: MeanReversionBoundaryState::try_new(
                    3.0, 2.0, 0.0, 1.0,
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
