use std::io::{Read, Write};
use std::mem::size_of;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::adapters::load_interval_factor_grid_for_calibration;
use crate::runtime_seasonality::RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD;
use crate::stages::joint_metrics::{
    JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY, JointRefinementAggregatedIntervalObservation,
    JointRefinementBusinessTimeBucketThresholds, JointRefinementIntervalObservationCursor,
    accumulate_return_into_intervals_with_cursor, finalize_sell_share_proxies,
    mean_absolute_return_by_utc_hour, mean_absolute_return_profile_from_hourly_totals,
    return_scale_summaries_by_business_time_bucket, return_scale_summary,
    return_scale_summary_from_returns, rolling_variance_activity_metric, sampled_return_values,
    scale_normalized_returns, tick_count_series,
};
use crate::stages::long_variance::metrics::{
    LongVarianceHorizonInputIntervals, aggregate_intervals_to_fixed_duration,
    long_variance_horizon_summaries,
};
use janiform_calibration_artifacts::{
    JointRefinementStageFittedParameters, SurpriseNewsDeterministicIntensityProfile,
};
use janiform_engine_simulation::{
    SeededFinePathRecord, SeededFinePathTransitionKind, SeededTickSimulationTransitionConsumer,
    estimate_seeded_result_capacities, stream_seeded_tick_event_simulation,
};
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::time::{NANOS_PER_DAY, NANOS_PER_HOUR};
use janiform_market_time::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact};
use janiform_market_time::interval_factor_grids::{
    IntervalFactorGrid, IntervalFactorGridProviderRequest, IntervalNewsIntensityFactorSource,
    IntervalSeasonalityFactorSource,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;

use super::super::execution::JointRefinementSeedSummary;
use super::super::mean_reversion_runtime::{
    endogenous_interval_factor_grids_from_interval_factor_grid,
    materialize_joint_refinement_mean_reversion_runtime_inputs,
};
use super::super::setup::build_surprise_multipliers_for_interval_grid;
use super::sampling::BoundedTickShapeReturnSampler;

pub const JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION: &str =
    "joint_refinement.summary_simulation.worker_protocol.v1";
const JOINT_REFINEMENT_SUMMARY_SIMULATION_FRAME_LENGTH_PREFIX_BYTES: usize = size_of::<u64>();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JointRefinementSummarySimulationMemoryEstimate {
    pub estimated_interval_template_bytes: usize,
    pub estimated_hourly_statistics_bytes: usize,
    pub estimated_trade_return_capacity_bytes: usize,
    pub estimated_total_retained_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct JointRefinementSummarySimulationWorkloadEstimate {
    pub(crate) estimated_transition_capacity: usize,
    pub(crate) estimated_event_capacity: usize,
    pub(crate) memory_estimate: JointRefinementSummarySimulationMemoryEstimate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementSummarySimulationSessionContext {
    pub calendar_registry_artifact: ExchangeCalendarRegistryArtifact,
    pub exchange_code: String,
    pub known_scheduled_event_timestamps_utc_nanos: Vec<i64>,
    pub runtime_interval_seasonality_factors: Vec<f64>,
    pub hourly_simulation_start_utc_nanos: i64,
    pub hourly_simulation_interval_count: usize,
    pub surprise_deterministic_intensity_profile: SurpriseNewsDeterministicIntensityProfile,
    pub simulated_5m_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
    pub simulated_15m_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
    pub simulated_hourly_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
    pub observed_5m_business_time_bucket_thresholds: JointRefinementBusinessTimeBucketThresholds,
    pub observed_15m_business_time_bucket_thresholds: JointRefinementBusinessTimeBucketThresholds,
    pub observed_tick_shape_target_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitializeSessionRequest {
    pub schema_version: String,
    pub session_context: JointRefinementSummarySimulationSessionContext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunCandidateRequest {
    pub schema_version: String,
    pub fitted_parameters: JointRefinementStageFittedParameters,
    pub requested_seed: u64,
    pub requested_output: JointRefinementWorkerRequestedOutput,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloseSessionRequest {
    pub schema_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JointRefinementWorkerRequestedOutput {
    ExactSummaryOnly,
    ExactSummaryWithDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementSummarySimulationSeedSummary {
    pub simulation_seed: u64,
    pub summary: JointRefinementSeedSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementSummarySimulationDiagnostics {
    pub memory_estimate: JointRefinementSummarySimulationMemoryEstimate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InitializeSessionResponse {
    Success {
        worker_baseline_resident_bytes: u64,
        worker_initialized_resident_bytes: u64,
    },
    InfrastructureFailure {
        error_message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RunCandidateResponse {
    ExactSummaryOnly {
        seed_summary: JointRefinementSummarySimulationSeedSummary,
    },
    ExactSummaryWithDiagnostics {
        seed_summary: JointRefinementSummarySimulationSeedSummary,
        diagnostics: JointRefinementSummarySimulationDiagnostics,
    },
    SimulationFailure {
        error_message: String,
    },
    InfrastructureFailure {
        error_message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloseSessionResponse {
    pub closed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JointRefinementSummarySimulationWorkerRequest {
    InitializeSession(Box<InitializeSessionRequest>),
    RunCandidate(Box<RunCandidateRequest>),
    CloseSession(Box<CloseSessionRequest>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JointRefinementSummarySimulationWorkerResponse {
    InitializeSession {
        schema_version: String,
        response: Box<InitializeSessionResponse>,
    },
    RunCandidate {
        schema_version: String,
        response: Box<RunCandidateResponse>,
    },
    CloseSession {
        schema_version: String,
        response: Box<CloseSessionResponse>,
    },
}

#[derive(Debug, Clone)]
pub struct JointRefinementSummarySimulationSessionState {
    calendar_registry: ExchangeCalendarRegistry,
    exchange_code: String,
    hourly_simulation_start_utc: DateTime<Utc>,
    hourly_simulation_interval_count: usize,
    surprise_deterministic_time_multipliers: Vec<f64>,
    interval_factor_grid_template: IntervalFactorGrid,
    simulated_5m_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_15m_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_hourly_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
    observed_5m_business_time_bucket_thresholds: JointRefinementBusinessTimeBucketThresholds,
    observed_15m_business_time_bucket_thresholds: JointRefinementBusinessTimeBucketThresholds,
    observed_tick_shape_target_count: usize,
}

pub fn write_joint_refinement_worker_frame<T: Serialize>(
    writer: &mut impl Write,
    message: &T,
) -> Result<(), String> {
    let payload = serialize_joint_refinement_worker_payload(message)?;

    let frame_length = u64::try_from(payload.len())
        .map_err(|_| "worker frame payload length exceeds u64".to_string())?;

    writer
        .write_all(frame_length.to_le_bytes().as_slice())
        .map_err(|source| format!("failed to write worker frame length prefix: {source}"))?;

    writer
        .write_all(payload.as_slice())
        .map_err(|source| format!("failed to write worker frame payload: {source}"))?;

    writer
        .flush()
        .map_err(|source| format!("failed to flush worker frame payload: {source}"))
}

pub fn read_joint_refinement_worker_frame<T: DeserializeOwned>(
    reader: &mut impl Read,
) -> Result<Option<T>, String> {
    let mut frame_length_prefix =
        [0_u8; JOINT_REFINEMENT_SUMMARY_SIMULATION_FRAME_LENGTH_PREFIX_BYTES];

    let first_prefix_byte_count = reader
        .read(&mut frame_length_prefix[..1])
        .map_err(|source| format!("failed to read worker frame length prefix: {source}"))?;
    if first_prefix_byte_count == 0 {
        return Ok(None);
    }

    reader
        .read_exact(&mut frame_length_prefix[1..])
        .map_err(|source| format!("failed to read worker frame length prefix: {source}"))?;

    let frame_length_u64 = u64::from_le_bytes(frame_length_prefix);

    let frame_length = usize::try_from(frame_length_u64)
        .map_err(|_| "worker frame length exceeds usize".to_string())?;

    let mut payload = vec![0_u8; frame_length];

    reader
        .read_exact(payload.as_mut_slice())
        .map_err(|source| format!("failed to read worker frame payload: {source}"))?;

    deserialize_joint_refinement_worker_payload(payload.as_slice()).map(Some)
}

fn serialize_joint_refinement_worker_payload<T: Serialize>(message: &T) -> Result<Vec<u8>, String> {
    postcard::to_allocvec(message)
        .map_err(|source| format!("failed to serialize worker frame: {source}"))
}

fn deserialize_joint_refinement_worker_payload<T: DeserializeOwned>(
    payload: &[u8],
) -> Result<T, String> {
    postcard::from_bytes(payload)
        .map_err(|source| format!("failed to deserialize worker frame payload: {source}"))
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementSummaryCollector {
    simulated_5m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_15m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_hourly_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_5m_cursor: JointRefinementIntervalObservationCursor,
    simulated_15m_cursor: JointRefinementIntervalObservationCursor,
    simulated_hourly_cursor: JointRefinementIntervalObservationCursor,
    tick_shape_return_sampler: BoundedTickShapeReturnSampler,
    trade_absolute_return_sum_by_hour: [f64; 24],
    trade_count_by_hour: [usize; 24],
}

impl JointRefinementSummaryCollector {
    pub(crate) fn new(
        simulated_5m_interval_template: &[JointRefinementAggregatedIntervalObservation],
        simulated_15m_interval_template: &[JointRefinementAggregatedIntervalObservation],
        simulated_hourly_interval_template: &[JointRefinementAggregatedIntervalObservation],
        retained_trade_return_target_count: usize,
    ) -> Self {
        Self {
            simulated_5m_intervals: simulated_5m_interval_template.to_vec(),
            simulated_15m_intervals: simulated_15m_interval_template.to_vec(),
            simulated_hourly_intervals: simulated_hourly_interval_template.to_vec(),
            simulated_5m_cursor: JointRefinementIntervalObservationCursor::default(),
            simulated_15m_cursor: JointRefinementIntervalObservationCursor::default(),
            simulated_hourly_cursor: JointRefinementIntervalObservationCursor::default(),
            tick_shape_return_sampler: BoundedTickShapeReturnSampler::new(
                retained_trade_return_target_count,
            ),
            trade_absolute_return_sum_by_hour: [0.0; 24],
            trade_count_by_hour: [0; 24],
        }
    }

    pub(crate) fn finish(
        mut self,
        observed_tick_shape_target_count: usize,
        observed_5m_business_time_bucket_thresholds: JointRefinementBusinessTimeBucketThresholds,
        observed_15m_business_time_bucket_thresholds: JointRefinementBusinessTimeBucketThresholds,
    ) -> JointRefinementSeedSummary {
        finalize_sell_share_proxies(
            &mut self.simulated_5m_intervals,
            JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY,
        );

        finalize_sell_share_proxies(
            &mut self.simulated_15m_intervals,
            JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY,
        );

        finalize_sell_share_proxies(
            &mut self.simulated_hourly_intervals,
            JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY,
        );

        let simulated_5m_log_returns = self
            .simulated_5m_intervals
            .iter()
            .map(|interval| interval.log_return)
            .collect::<Vec<_>>();

        let simulated_15m_log_returns = self
            .simulated_15m_intervals
            .iter()
            .map(|interval| interval.log_return)
            .collect::<Vec<_>>();

        let simulated_1h_log_returns = self
            .simulated_hourly_intervals
            .iter()
            .map(|interval| interval.log_return)
            .collect::<Vec<_>>();

        let simulated_1d_intervals = aggregate_intervals_to_fixed_duration(
            self.simulated_hourly_intervals.as_slice(),
            NANOS_PER_DAY,
        );

        let simulated_1d_log_returns = simulated_1d_intervals
            .iter()
            .map(|interval| interval.log_return)
            .collect::<Vec<_>>();

        let simulated_tick_returns = sampled_return_values(
            self.tick_shape_return_sampler.retained_returns(),
            observed_tick_shape_target_count,
        );

        let simulated_5m_return_scale_summary =
            return_scale_summary(self.simulated_5m_intervals.as_slice());

        let simulated_15m_return_scale_summary =
            return_scale_summary(self.simulated_15m_intervals.as_slice());

        let simulated_1h_return_scale_summary =
            return_scale_summary(self.simulated_hourly_intervals.as_slice());

        let simulated_1d_return_scale_summary =
            return_scale_summary(simulated_1d_intervals.as_slice());

        let simulated_5m_business_time_return_scale_summaries =
            return_scale_summaries_by_business_time_bucket(
                self.simulated_5m_intervals.as_slice(),
                observed_5m_business_time_bucket_thresholds,
            );

        let simulated_15m_business_time_return_scale_summaries =
            return_scale_summaries_by_business_time_bucket(
                self.simulated_15m_intervals.as_slice(),
                observed_15m_business_time_bucket_thresholds,
            );

        let simulated_tick_intraday_mean_absolute_returns = mean_absolute_trade_return_profile(
            self.trade_absolute_return_sum_by_hour,
            self.trade_count_by_hour,
        );

        let simulated_5m_intraday_mean_absolute_returns =
            mean_absolute_return_by_utc_hour(self.simulated_5m_intervals.as_slice());

        let simulated_15m_intraday_mean_absolute_returns =
            mean_absolute_return_by_utc_hour(self.simulated_15m_intervals.as_slice());

        let simulated_hourly_tick_counts =
            tick_count_series(self.simulated_hourly_intervals.as_slice());

        let simulated_15m_rv_activity_metric =
            rolling_variance_activity_metric(self.simulated_15m_intervals.as_slice());

        let simulated_long_variance_horizon_summaries =
            long_variance_horizon_summaries(LongVarianceHorizonInputIntervals {
                five_minute_return_intervals: self.simulated_5m_intervals.as_slice(),
                five_minute_activity_intervals: self.simulated_5m_intervals.as_slice(),
                hourly_return_intervals: self.simulated_hourly_intervals.as_slice(),
                hourly_activity_intervals: self.simulated_hourly_intervals.as_slice(),
            });

        JointRefinementSeedSummary {
            simulated_5m_intervals: self.simulated_5m_intervals,
            simulated_15m_intervals: self.simulated_15m_intervals,
            simulated_tick_shape_returns: scale_normalized_returns(
                simulated_tick_returns.as_slice(),
            ),
            simulated_5m_shape_returns: scale_normalized_returns(
                simulated_5m_log_returns.as_slice(),
            ),
            simulated_15m_shape_returns: scale_normalized_returns(
                simulated_15m_log_returns.as_slice(),
            ),
            simulated_1h_shape_returns: scale_normalized_returns(
                simulated_1h_log_returns.as_slice(),
            ),
            simulated_1d_shape_returns: scale_normalized_returns(
                simulated_1d_log_returns.as_slice(),
            ),
            simulated_tick_return_scale_summary: return_scale_summary_from_returns(
                simulated_tick_returns.as_slice(),
            ),
            simulated_5m_return_scale_summary,
            simulated_15m_return_scale_summary,
            simulated_1h_return_scale_summary,
            simulated_1d_return_scale_summary,
            simulated_5m_business_time_return_scale_summaries,
            simulated_15m_business_time_return_scale_summaries,
            simulated_tick_intraday_mean_absolute_returns,
            simulated_5m_intraday_mean_absolute_returns,
            simulated_15m_intraday_mean_absolute_returns,
            simulated_hourly_tick_counts,
            simulated_15m_rv_activity_metric,
            simulated_long_variance_horizon_summaries,
        }
    }
}

impl SeededTickSimulationTransitionConsumer for JointRefinementSummaryCollector {
    fn reserve_transition_capacity(
        &mut self,
        _estimated_transition_capacity: usize,
        estimated_event_capacity: usize,
    ) -> Result<(), janiform_engine_simulation::SeededTickSimulationRunError> {
        self.tick_shape_return_sampler
            .reserve_retention_capacity(estimated_event_capacity)
    }

    fn consume_transition(
        &mut self,
        record: SeededFinePathRecord,
        _emission: janiform_engine_simulation::SeededTickSimulationTransitionEmission,
    ) -> Result<(), janiform_engine_simulation::SeededTickSimulationRunError> {
        let (tick_increment, sell_share_increment) = match record.transition_kind {
            SeededFinePathTransitionKind::BuyTrade => (1, 0.0),
            SeededFinePathTransitionKind::SellTrade => (1, 1.0),
            _ => (0, 0.0),
        };

        accumulate_return_into_intervals_with_cursor(
            record.observed_timestamp_utc_nanos,
            record.return_components.total_return,
            tick_increment,
            sell_share_increment,
            self.simulated_5m_intervals.as_mut_slice(),
            &mut self.simulated_5m_cursor,
        );

        accumulate_return_into_intervals_with_cursor(
            record.observed_timestamp_utc_nanos,
            record.return_components.total_return,
            tick_increment,
            sell_share_increment,
            self.simulated_15m_intervals.as_mut_slice(),
            &mut self.simulated_15m_cursor,
        );

        accumulate_return_into_intervals_with_cursor(
            record.observed_timestamp_utc_nanos,
            record.return_components.total_return,
            tick_increment,
            sell_share_increment,
            self.simulated_hourly_intervals.as_mut_slice(),
            &mut self.simulated_hourly_cursor,
        );

        if tick_increment > 0 {
            self.tick_shape_return_sampler
                .retain_return(record.return_components.total_return);

            let hour_index = record
                .observed_timestamp_utc_nanos
                .div_euclid(NANOS_PER_HOUR)
                .rem_euclid(24) as usize;

            self.trade_absolute_return_sum_by_hour[hour_index] +=
                record.return_components.total_return.abs();

            self.trade_count_by_hour[hour_index] += 1;
        }

        Ok(())
    }
}

pub fn initialize_joint_refinement_summary_simulation_session(
    session_context: &JointRefinementSummarySimulationSessionContext,
) -> Result<JointRefinementSummarySimulationSessionState, String> {
    let calendar_registry =
        ExchangeCalendarRegistry::from_artifact(session_context.calendar_registry_artifact.clone())
            .map_err(|source| source.to_string())?;

    let hourly_simulation_start_utc =
        DateTime::<Utc>::from_timestamp_nanos(session_context.hourly_simulation_start_utc_nanos);

    let _hourly_simulation_end_utc = hourly_simulation_start_utc
        .checked_add_signed(Duration::hours(
            session_context.hourly_simulation_interval_count as i64,
        ))
        .ok_or_else(|| {
            "joint refinement hourly simulation end timestamp is out of range".to_string()
        })?;

    let surprise_deterministic_time_multiplier_by_interval_index =
        build_surprise_multipliers_for_interval_grid(
            hourly_simulation_start_utc,
            session_context.hourly_simulation_interval_count,
            NANOS_PER_HOUR,
            &session_context.surprise_deterministic_intensity_profile,
        )?;

    let surprise_deterministic_time_multipliers =
        surprise_deterministic_time_multiplier_by_interval_index
            .iter()
            .map(|(_, multiplier)| *multiplier)
            .collect::<Vec<_>>();

    let hourly_simulation_end_utc = hourly_simulation_start_utc
        .checked_add_signed(Duration::hours(
            session_context.hourly_simulation_interval_count as i64,
        ))
        .ok_or_else(|| {
            "joint refinement hourly simulation end timestamp is out of range".to_string()
        })?;

    let interval_factor_grid_template = load_interval_factor_grid_for_calibration(
        &calendar_registry,
        IntervalFactorGridProviderRequest {
            exchange_code: session_context.exchange_code.as_str(),
            range_start_utc: hourly_simulation_start_utc,
            range_end_utc: hourly_simulation_end_utc,
            interval_duration_nanos: NANOS_PER_HOUR,
            seasonality_source: Some(IntervalSeasonalityFactorSource::Explicit {
                interval_factors: session_context
                    .runtime_interval_seasonality_factors
                    .as_slice(),
                materialization_method: RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
            }),
            news_intensity_source: Some(IntervalNewsIntensityFactorSource::Materialized {
                known_scheduled_event_timestamps_utc_nanos: session_context
                    .known_scheduled_event_timestamps_utc_nanos
                    .as_slice(),
                surprise_open_baseline_intensity: 0.0,
                surprise_closed_baseline_intensity: 0.0,
                surprise_deterministic_time_multiplier_by_interval_index:
                    surprise_deterministic_time_multiplier_by_interval_index.as_slice(),
            }),
        },
    )
    .map_err(|source| source.to_string())?;

    Ok(JointRefinementSummarySimulationSessionState {
        calendar_registry,
        exchange_code: session_context.exchange_code.clone(),
        hourly_simulation_start_utc,
        hourly_simulation_interval_count: session_context.hourly_simulation_interval_count,
        surprise_deterministic_time_multipliers,
        interval_factor_grid_template,
        simulated_5m_interval_template: session_context.simulated_5m_interval_template.clone(),
        simulated_15m_interval_template: session_context.simulated_15m_interval_template.clone(),
        simulated_hourly_interval_template: session_context
            .simulated_hourly_interval_template
            .clone(),
        observed_5m_business_time_bucket_thresholds: session_context
            .observed_5m_business_time_bucket_thresholds,
        observed_15m_business_time_bucket_thresholds: session_context
            .observed_15m_business_time_bucket_thresholds,
        observed_tick_shape_target_count: session_context.observed_tick_shape_target_count,
    })
}

pub fn simulate_joint_refinement_seed_summary_in_process(
    session_state: &JointRefinementSummarySimulationSessionState,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
    cancellation_token: CancellationToken,
) -> Result<
    (
        JointRefinementSeedSummary,
        JointRefinementSummarySimulationMemoryEstimate,
    ),
    String,
> {
    let run_config = build_seeded_summary_simulation_run_config(
        session_state,
        fitted_parameters,
        simulation_seed,
    )?;

    let (estimated_transition_capacity, estimated_event_capacity) =
        estimate_seeded_result_capacities(&run_config);

    let memory_estimate =
        summary_simulation_memory_estimate_from_session(session_state, estimated_event_capacity);

    let mut collector = JointRefinementSummaryCollector::new(
        session_state.simulated_5m_interval_template.as_slice(),
        session_state.simulated_15m_interval_template.as_slice(),
        session_state.simulated_hourly_interval_template.as_slice(),
        session_state.observed_tick_shape_target_count,
    );

    collector
        .reserve_transition_capacity(estimated_transition_capacity, estimated_event_capacity)
        .map_err(|source| source.to_string())?;

    stream_seeded_tick_event_simulation(run_config, cancellation_token, &mut collector)
        .map_err(|source| source.to_string())?;

    Ok((
        collector.finish(
            session_state.observed_tick_shape_target_count,
            session_state.observed_5m_business_time_bucket_thresholds,
            session_state.observed_15m_business_time_bucket_thresholds,
        ),
        memory_estimate,
    ))
}

pub fn run_joint_refinement_candidate_in_session(
    session_state: &JointRefinementSummarySimulationSessionState,
    request: &RunCandidateRequest,
) -> RunCandidateResponse {
    if request.schema_version != JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
    {
        return RunCandidateResponse::InfrastructureFailure {
            error_message: format!(
                "unexpected worker request schema version: {}",
                request.schema_version
            ),
        };
    }

    // Process-worker cancellation is owned by the parent scheduler, which
    // terminates this child process when a request is cancelled.
    let cancellation_token = CancellationToken::uncancelled();

    let (summary, memory_estimate) = match simulate_joint_refinement_seed_summary_in_process(
        session_state,
        &request.fitted_parameters,
        request.requested_seed,
        cancellation_token,
    ) {
        Ok(run_result) => run_result,
        Err(error_message) => {
            return RunCandidateResponse::SimulationFailure {
                error_message: format!(
                    "joint refinement summary simulation failed for seed {}: {}",
                    request.requested_seed, error_message
                ),
            };
        }
    };

    let seed_summary = JointRefinementSummarySimulationSeedSummary {
        simulation_seed: request.requested_seed,
        summary,
    };

    match request.requested_output {
        JointRefinementWorkerRequestedOutput::ExactSummaryOnly => {
            RunCandidateResponse::ExactSummaryOnly { seed_summary }
        }
        JointRefinementWorkerRequestedOutput::ExactSummaryWithDiagnostics => {
            RunCandidateResponse::ExactSummaryWithDiagnostics {
                seed_summary,
                diagnostics: JointRefinementSummarySimulationDiagnostics { memory_estimate },
            }
        }
    }
}

pub(crate) fn estimate_summary_simulation_memory(
    session_state: &JointRefinementSummarySimulationSessionState,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
) -> Result<JointRefinementSummarySimulationMemoryEstimate, String> {
    estimate_summary_simulation_workload(session_state, fitted_parameters, simulation_seed)
        .map(|workload_estimate| workload_estimate.memory_estimate)
}

pub(crate) fn estimate_summary_simulation_workload(
    session_state: &JointRefinementSummarySimulationSessionState,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
) -> Result<JointRefinementSummarySimulationWorkloadEstimate, String> {
    let run_config = build_seeded_summary_simulation_run_config(
        session_state,
        fitted_parameters,
        simulation_seed,
    )?;

    let (estimated_transition_capacity, estimated_event_capacity) =
        estimate_seeded_result_capacities(&run_config);

    let memory_estimate =
        summary_simulation_memory_estimate_from_session(session_state, estimated_event_capacity);

    Ok(JointRefinementSummarySimulationWorkloadEstimate {
        estimated_transition_capacity,
        estimated_event_capacity,
        memory_estimate,
    })
}

fn build_seeded_summary_simulation_run_config(
    session_state: &JointRefinementSummarySimulationSessionState,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
) -> Result<janiform_engine_simulation::SeededTickSimulationRunConfig, String> {
    let surprise_arrival_configuration = fitted_parameters
        .return_components_fitted_parameters
        .news_process_configuration
        .arrivals
        .surprise;

    let surprise_news_intensity = session_state
        .surprise_deterministic_time_multipliers
        .iter()
        .enumerate()
        .map(|(interval_position, deterministic_time_multiplier)| {
            let market_state_exposure = session_state
                .calendar_registry
                .resolve_market_state_exposure_in_utc_interval(
                    session_state.exchange_code.as_str(),
                    session_state
                        .interval_factor_grid_template
                        .interval_start_utc_nanos[interval_position],
                    session_state
                        .interval_factor_grid_template
                        .interval_end_utc_nanos[interval_position],
                )
                .map_err(|source| source.to_string())?;

            Ok((surprise_arrival_configuration.open_baseline_intensity
                * market_state_exposure.open_window_share
                + surprise_arrival_configuration.closed_baseline_intensity
                    * market_state_exposure.closed_window_share)
                * deterministic_time_multiplier)
        })
        .collect::<Result<Vec<_>, String>>()?;

    let news_intensity = session_state
        .interval_factor_grid_template
        .known_news_intensity
        .iter()
        .zip(surprise_news_intensity.iter())
        .map(|(known_news_intensity, surprise_news_intensity)| {
            known_news_intensity + surprise_news_intensity
        })
        .collect::<Vec<_>>();

    let mut interval_factor_grid = session_state.interval_factor_grid_template.clone();

    interval_factor_grid.surprise_news_intensity = surprise_news_intensity;
    interval_factor_grid.news_intensity = news_intensity;

    let initial_excitation_state = HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    };

    let endogenous_interval_factor_grids =
        endogenous_interval_factor_grids_from_interval_factor_grid(&interval_factor_grid)?;

    let (mean_reversion_runtime_interval_anchors, initial_mean_reversion_boundary_state) =
        materialize_joint_refinement_mean_reversion_runtime_inputs(
            interval_factor_grid.metadata.interval_alignment_policy,
            fitted_parameters,
            initial_excitation_state,
            endogenous_interval_factor_grids.as_slice(),
        )?;

    janiform_engine_simulation::SeededTickSimulationRunConfig::try_new(
        simulation_seed,
        session_state.hourly_simulation_start_utc,
        session_state.hourly_simulation_interval_count,
        janiform_engine_simulation::SeededTickSimulationModelInputs {
            kernel_parameters: fitted_parameters.hawkes_fitted_parameters.kernel_parameters,
            nig_shared_plus_shift_configuration: fitted_parameters
                .return_components_fitted_parameters
                .nig_shared_plus_shift_configuration,
            news_process_configuration: fitted_parameters
                .return_components_fitted_parameters
                .news_process_configuration
                .clone(),
            initial_excitation_state,
            mean_reversion_fitted_parameters: fitted_parameters
                .return_components_fitted_parameters
                .mean_reversion_fitted_parameters
                .clone(),
            mean_reversion_runtime_interval_anchors,
            initial_mean_reversion_boundary_state,
            interval_factor_grid,
            return_clock_exposure_parameters: fitted_parameters
                .return_components_fitted_parameters
                .return_clock_exposure_parameters,
            long_variance_parameters: fitted_parameters
                .return_components_fitted_parameters
                .long_variance_parameters,
            initial_long_variance_state:
                janiform_model_kernels::long_variance::LongVarianceState::unit(),
            observed_price_tick_size: fitted_parameters
                .return_components_fitted_parameters
                .tick_grid_observation_model
                .tick_size,
            timestamp_batching_observation_model: fitted_parameters
                .return_components_fitted_parameters
                .timestamp_batching_observation_model
                .clone(),
            calendar_registry: session_state.calendar_registry.clone(),
        },
        Some(janiform_engine_simulation::SeededTickSimulationRunOptions {
            debug_trace_enabled: false,
            debug_trace_options: Default::default(),
            state_snapshot_capture:
                janiform_engine_simulation::SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .map_err(|source| source.to_string())
}

fn summary_simulation_memory_estimate_from_session(
    session_state: &JointRefinementSummarySimulationSessionState,
    _estimated_event_capacity: usize,
) -> JointRefinementSummarySimulationMemoryEstimate {
    let estimated_interval_template_bytes = session_state
        .simulated_5m_interval_template
        .len()
        .saturating_mul(size_of::<JointRefinementAggregatedIntervalObservation>())
        .saturating_add(
            session_state
                .simulated_15m_interval_template
                .len()
                .saturating_mul(size_of::<JointRefinementAggregatedIntervalObservation>()),
        )
        .saturating_add(
            session_state
                .simulated_hourly_interval_template
                .len()
                .saturating_mul(size_of::<JointRefinementAggregatedIntervalObservation>()),
        );

    let estimated_hourly_statistics_bytes = size_of::<[f64; 24]>() + size_of::<[usize; 24]>();

    // The streaming tick-shape sampler reserves the observed shape target, not
    // the simulated event count.
    let estimated_trade_return_capacity = session_state.observed_tick_shape_target_count;

    let estimated_trade_return_capacity_bytes =
        estimated_trade_return_capacity.saturating_mul(size_of::<f64>());

    JointRefinementSummarySimulationMemoryEstimate {
        estimated_interval_template_bytes,
        estimated_hourly_statistics_bytes,
        estimated_trade_return_capacity_bytes,
        estimated_total_retained_bytes: estimated_interval_template_bytes
            .saturating_add(estimated_hourly_statistics_bytes)
            .saturating_add(estimated_trade_return_capacity_bytes),
    }
}

fn mean_absolute_trade_return_profile(
    absolute_return_sum_by_hour: [f64; 24],
    observation_count_by_hour: [usize; 24],
) -> Vec<f64> {
    mean_absolute_return_profile_from_hourly_totals(
        absolute_return_sum_by_hour,
        observation_count_by_hour,
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::LazyLock;
    use std::time::SystemTime;

    use chrono::{DateTime, Duration};
    use janiform_foundations::cancellation::CancellationToken;

    use super::{
        CloseSessionRequest, InitializeSessionRequest, InitializeSessionResponse,
        JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION,
        JointRefinementSummarySimulationSessionContext,
        JointRefinementSummarySimulationSessionState,
        JointRefinementSummarySimulationWorkerRequest,
        JointRefinementSummarySimulationWorkerResponse, JointRefinementWorkerRequestedOutput,
        RunCandidateRequest, RunCandidateResponse, build_seeded_summary_simulation_run_config,
        deserialize_joint_refinement_worker_payload,
        initialize_joint_refinement_summary_simulation_session, read_joint_refinement_worker_frame,
        serialize_joint_refinement_worker_payload,
        simulate_joint_refinement_seed_summary_in_process, write_joint_refinement_worker_frame,
    };
    use crate::load_interval_factor_grid_for_calibration;
    use crate::stages::hawkes::{
        HawkesStageCandidateInput, HawkesStageDirectionalLabelDiagnostics,
        HawkesStageExecutionInput, HawkesStageFittedPayload, HawkesStageObservedSourceRegime,
        run_hawkes_calibration_stage,
    };
    use crate::stages::joint_metrics::{
        aggregate_simulated_path_to_interval_template, mean_absolute_trade_return_by_utc_hour,
        return_scale_summaries_by_business_time_bucket, return_scale_summary,
        return_scale_summary_from_returns, rolling_variance_activity_metric, sampled_trade_returns,
        scale_normalized_returns, tick_count_series,
    };
    use crate::stages::joint_refinement::{
        JointRefinementSeedSummary, build_surprise_multipliers_for_interval_grid,
    };
    use crate::stages::long_variance::metrics::{
        LongVarianceHorizonInputIntervals, aggregate_intervals_to_fixed_duration,
        long_variance_horizon_summaries,
    };
    use crate::stages::return_components::{
        ReturnComponentsCloseOpenGapDetectionPolicy, ReturnComponentsDetectedIntradayJumpCandidate,
        ReturnComponentsKnownNewsCalendarInput,
        ReturnComponentsObservedCloseOpenBoundaryObservation,
        ReturnComponentsObservedOpenSessionReturn, ReturnComponentsRealizedKnownNewsReturnSample,
        ReturnComponentsStageCandidateGenerationInput, ReturnComponentsStageExecutionInput,
        ReturnComponentsStageFittedPayload, ReturnComponentsStageObjectiveMetricWeights,
        ReturnComponentsStageSignalSourceWindow,
        prepare_observed_signal_inputs_for_joint_refinement, resolve_activity_calibration_context,
        run_return_components_calibration_stage,
    };
    use crate::stages::seasonality::{
        CalibrationStageExecutionStatus, RuntimeIntervalSeasonalityFactorsBuildInput,
        SeasonalityStageExecutionInput, SeasonalityStageNormalizedPayload,
        build_runtime_interval_seasonality_factors_from_components,
        run_seasonality_calibration_stage,
    };
    use crate::stages::{
        joint_refinement::execution::{
            JointRefinementObservedDataBuildInput, build_exchange_calendar_registry_artifact,
            build_observed_data,
        },
        joint_refinement::setup::selected_hawkes_observed_open_event_rate_per_hour,
    };
    use janiform_calibration_artifacts::{
        CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationRunDataInputs,
        CalibrationRunSeedSettings, CalibrationRunSpec, CalibrationStage, CalibrationStageHandoff,
        CalibrationStageSeedOverride, CalibrationStageSolverSettings,
        JointRefinementStageFittedParameters,
    };
    use janiform_engine_simulation::run_seeded_tick_event_simulation;
    use janiform_foundations::time::{NANOS_PER_DAY, NANOS_PER_HOUR, utc_nanos};
    use janiform_market_time::calendar::{
        ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader, TimestampEventInputRecord,
    };
    use janiform_market_time::interval_factor_grids::{
        IntervalFactorGridProviderRequest, IntervalNewsIntensityFactorSource,
        IntervalSeasonalityFactorSource,
    };
    use janiform_model_kernels::hawkes::{
        HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
        HawkesStructuredRhoSelfDominancePolicy,
    };
    use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;
    use janiform_model_kernels::news::{
        GammaSeverityMarkConfiguration, SharedNewsMarkLawConfiguration,
    };
    use janiform_model_kernels::return_clock::SideSourceActivityDecomposition;

    #[derive(Debug, Clone, PartialEq)]
    struct JointRefinementSummarySimulationFixture {
        session_context: JointRefinementSummarySimulationSessionContext,
        fitted_parameters: JointRefinementStageFittedParameters,
        simulation_seed: u64,
    }

    #[test]
    fn summary_only_seed_simulation_matches_full_path_derived_summary() {
        let fixture = joint_refinement_summary_fixture();

        let session_state =
            initialize_joint_refinement_summary_simulation_session(&fixture.session_context)
                .expect("summary simulation session should initialize");

        let (summary_only, memory_estimate) = simulate_joint_refinement_seed_summary_in_process(
            &session_state,
            &fixture.fitted_parameters,
            fixture.simulation_seed,
            CancellationToken::uncancelled(),
        )
        .expect("summary-only simulation should succeed");

        let (full_path_summary, full_path_native_diagnostics) =
            full_path_seed_summary_fixture(&fixture, &session_state);

        let comparable_summary_only = summary_only.clone();

        let mut comparable_full_path_summary = full_path_summary.clone();

        comparable_full_path_summary.simulated_tick_shape_returns =
            comparable_summary_only.simulated_tick_shape_returns.clone();
        comparable_full_path_summary.simulated_tick_return_scale_summary =
            comparable_summary_only.simulated_tick_return_scale_summary;

        assert_eq!(comparable_summary_only, comparable_full_path_summary);
        assert!(
            summary_only.simulated_tick_shape_returns.len()
                <= fixture.session_context.observed_tick_shape_target_count
        );
        assert!(
            memory_estimate.estimated_total_retained_bytes
                < full_path_native_diagnostics.estimated_total_retained_bytes,
            "summary-only retained bytes should stay below full-path retained bytes",
        );
    }

    #[test]
    fn worker_process_summary_matches_in_process_summary() {
        let fixture = joint_refinement_summary_fixture();

        let session_state =
            initialize_joint_refinement_summary_simulation_session(&fixture.session_context)
                .expect("summary simulation session should initialize");

        let (expected_summary, expected_memory_estimate) =
            simulate_joint_refinement_seed_summary_in_process(
                &session_state,
                &fixture.fitted_parameters,
                fixture.simulation_seed,
                CancellationToken::uncancelled(),
            )
            .expect("summary-only simulation should succeed");

        let run_candidate_request = RunCandidateRequest {
            schema_version: JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                .to_string(),
            fitted_parameters: fixture.fitted_parameters.clone(),
            requested_seed: fixture.simulation_seed,
            requested_output: JointRefinementWorkerRequestedOutput::ExactSummaryWithDiagnostics,
        };

        let serialized_request = serialize_joint_refinement_worker_payload(&run_candidate_request)
            .expect("summary simulation run-candidate request should serialize");

        let round_tripped_request: RunCandidateRequest =
            deserialize_joint_refinement_worker_payload(serialized_request.as_slice())
                .expect("summary simulation run-candidate request should deserialize");

        assert_eq!(round_tripped_request, run_candidate_request);

        let (round_tripped_summary, round_tripped_memory_estimate) =
            simulate_joint_refinement_seed_summary_in_process(
                &session_state,
                &round_tripped_request.fitted_parameters,
                fixture.simulation_seed,
                CancellationToken::uncancelled(),
            )
            .expect("round-tripped summary-only simulation should succeed");

        assert_eq!(round_tripped_summary, expected_summary);
        assert_eq!(round_tripped_memory_estimate, expected_memory_estimate);

        let worker_response =
            spawn_worker_summary_response(&fixture.session_context, &run_candidate_request);

        match worker_response {
            JointRefinementSummarySimulationWorkerResponse::RunCandidate { response, .. } => {
                match *response {
                    RunCandidateResponse::ExactSummaryWithDiagnostics {
                        seed_summary,
                        diagnostics,
                    } => {
                        assert_eq!(seed_summary.simulation_seed, fixture.simulation_seed);
                        assert_eq!(seed_summary.summary, expected_summary);
                        assert_eq!(diagnostics.memory_estimate, expected_memory_estimate);
                    }
                    RunCandidateResponse::ExactSummaryOnly { .. } => {
                        panic!("diagnostic worker request should return diagnostics");
                    }
                    RunCandidateResponse::SimulationFailure { error_message } => {
                        panic!("worker summary simulation failed: {error_message}");
                    }
                    RunCandidateResponse::InfrastructureFailure { error_message } => {
                        panic!("worker infrastructure failed: {error_message}");
                    }
                }
            }
            _ => {
                panic!("run-candidate request should return a run-candidate response");
            }
        }
    }

    #[test]
    fn run_config_interval_factor_grid_matches_materialized_provider_output() {
        let fixture = joint_refinement_summary_fixture();

        let session_state =
            initialize_joint_refinement_summary_simulation_session(&fixture.session_context)
                .expect("summary simulation session should initialize");

        let run_config = build_seeded_summary_simulation_run_config(
            &session_state,
            &fixture.fitted_parameters,
            fixture.simulation_seed,
        )
        .expect("summary simulation run config should build");

        let hourly_simulation_end_utc = session_state
            .hourly_simulation_start_utc
            .checked_add_signed(Duration::hours(
                session_state.hourly_simulation_interval_count as i64,
            ))
            .expect("hourly simulation end should stay in range");

        let surprise_deterministic_time_multiplier_by_interval_index =
            build_surprise_multipliers_for_interval_grid(
                session_state.hourly_simulation_start_utc,
                session_state.hourly_simulation_interval_count,
                NANOS_PER_HOUR,
                &fixture
                    .session_context
                    .surprise_deterministic_intensity_profile,
            )
            .expect("test surprise profile materialization should succeed");

        let expected_interval_factor_grid = load_interval_factor_grid_for_calibration(
            &session_state.calendar_registry,
            IntervalFactorGridProviderRequest {
                exchange_code: fixture.session_context.exchange_code.as_str(),
                range_start_utc: session_state.hourly_simulation_start_utc,
                range_end_utc: hourly_simulation_end_utc,
                interval_duration_nanos: NANOS_PER_HOUR,
                seasonality_source: Some(IntervalSeasonalityFactorSource::Explicit {
                    interval_factors: fixture
                        .session_context
                        .runtime_interval_seasonality_factors
                        .as_slice(),
                    materialization_method: crate::runtime_seasonality::RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
                }),
                news_intensity_source: Some(IntervalNewsIntensityFactorSource::Materialized {
                    known_scheduled_event_timestamps_utc_nanos: fixture
                        .session_context
                        .known_scheduled_event_timestamps_utc_nanos
                        .as_slice(),
                    surprise_open_baseline_intensity: fixture
                        .fitted_parameters
                        .return_components_fitted_parameters
                        .news_process_configuration
                        .arrivals
                        .surprise
                        .open_baseline_intensity,
                    surprise_closed_baseline_intensity: fixture
                        .fitted_parameters
                        .return_components_fitted_parameters
                        .news_process_configuration
                        .arrivals
                        .surprise
                        .closed_baseline_intensity,
                    surprise_deterministic_time_multiplier_by_interval_index:
                        surprise_deterministic_time_multiplier_by_interval_index.as_slice(),
                }),
            },
        )
        .expect("expected interval factor grid contract should build");

        assert_eq!(
            run_config.model_inputs.interval_factor_grid,
            expected_interval_factor_grid,
        );
    }

    #[test]
    fn exact_summary_only_response_omits_diagnostics_entirely() {
        let fixture = joint_refinement_summary_fixture();

        let worker_response = spawn_worker_summary_response(
            &fixture.session_context,
            &RunCandidateRequest {
                schema_version: JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                    .to_string(),
                fitted_parameters: fixture.fitted_parameters.clone(),
                requested_seed: fixture.simulation_seed,
                requested_output: JointRefinementWorkerRequestedOutput::ExactSummaryOnly,
            },
        );

        match worker_response {
            JointRefinementSummarySimulationWorkerResponse::RunCandidate { response, .. } => {
                assert!(matches!(
                    response.as_ref(),
                    RunCandidateResponse::ExactSummaryOnly { .. }
                ));
            }
            _ => panic!("run-candidate request should return a run-candidate response"),
        }
    }

    #[test]
    fn run_config_preserves_candidate_mean_reversion_parameters() {
        let mut fixture = joint_refinement_summary_fixture();

        let mean_reversion_fitted_parameters =
            MeanReversionFittedParameters::endogenous_core_pressure_level(0.25, 1.0, 0.5, 0.75)
                .expect("mean-reversion fixture parameters should validate");
        fixture
            .fitted_parameters
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters = mean_reversion_fitted_parameters.clone();

        let session_state =
            initialize_joint_refinement_summary_simulation_session(&fixture.session_context)
                .expect("summary simulation session should initialize");

        let run_config = build_seeded_summary_simulation_run_config(
            &session_state,
            &fixture.fitted_parameters,
            fixture.simulation_seed,
        )
        .expect("summary simulation run config should build");

        assert_eq!(
            run_config.model_inputs.mean_reversion_fitted_parameters,
            mean_reversion_fitted_parameters
        );
    }

    #[test]
    fn worker_session_reuses_initialized_state_across_multiple_candidate_requests() {
        let fixture = joint_refinement_summary_fixture();
        let worker_executable = worker_executable_path();

        let mut child = spawn_worker_process(worker_executable.as_path());

        let (mut stdin, mut stdout) = worker_stdio(&mut child);

        write_joint_refinement_worker_frame(
            &mut stdin,
            &JointRefinementSummarySimulationWorkerRequest::InitializeSession(Box::new(
                InitializeSessionRequest {
                    schema_version:
                        JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                            .to_string(),
                    session_context: fixture.session_context.clone(),
                },
            )),
        )
        .expect("initialize-session request should write");

        let initialize_response = read_joint_refinement_worker_frame::<
            JointRefinementSummarySimulationWorkerResponse,
        >(&mut stdout)
        .expect("initialize-session response should deserialize")
        .expect("initialize-session response should exist");

        match initialize_response {
            JointRefinementSummarySimulationWorkerResponse::InitializeSession {
                response, ..
            } => {
                assert!(matches!(
                    response.as_ref(),
                    InitializeSessionResponse::Success { .. }
                ));
            }
            _ => panic!("initialize-session should return initialize-session response"),
        }

        let run_request = JointRefinementSummarySimulationWorkerRequest::RunCandidate(Box::new(
            RunCandidateRequest {
                schema_version: JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                    .to_string(),
                fitted_parameters: fixture.fitted_parameters.clone(),
                requested_seed: fixture.simulation_seed,
                requested_output: JointRefinementWorkerRequestedOutput::ExactSummaryOnly,
            },
        ));

        write_joint_refinement_worker_frame(&mut stdin, &run_request)
            .expect("first run-candidate request should write");

        let first_response = read_joint_refinement_worker_frame::<
            JointRefinementSummarySimulationWorkerResponse,
        >(&mut stdout)
        .expect("first run-candidate response should deserialize")
        .expect("first run-candidate response should exist");

        assert!(matches!(
            first_response,
            JointRefinementSummarySimulationWorkerResponse::RunCandidate {
                response,
                ..
            } if matches!(response.as_ref(), RunCandidateResponse::ExactSummaryOnly { .. })
        ));

        write_joint_refinement_worker_frame(&mut stdin, &run_request)
            .expect("second run-candidate request should write");

        let second_response = read_joint_refinement_worker_frame::<
            JointRefinementSummarySimulationWorkerResponse,
        >(&mut stdout)
        .expect("second run-candidate response should deserialize")
        .expect("second run-candidate response should exist");

        assert!(matches!(
            second_response,
            JointRefinementSummarySimulationWorkerResponse::RunCandidate {
                response,
                ..
            } if matches!(response.as_ref(), RunCandidateResponse::ExactSummaryOnly { .. })
        ));

        write_joint_refinement_worker_frame(
            &mut stdin,
            &JointRefinementSummarySimulationWorkerRequest::CloseSession(Box::new(
                CloseSessionRequest {
                    schema_version:
                        JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                            .to_string(),
                },
            )),
        )
        .expect("close-session request should write");

        let close_response = read_joint_refinement_worker_frame::<
            JointRefinementSummarySimulationWorkerResponse,
        >(&mut stdout)
        .expect("close-session response should deserialize")
        .expect("close-session response should exist");

        assert!(matches!(
            close_response,
            JointRefinementSummarySimulationWorkerResponse::CloseSession { .. }
        ));
    }

    #[test]
    fn run_candidate_request_is_smaller_than_initialize_session_request() {
        let fixture = joint_refinement_summary_fixture();

        let initialize_request = InitializeSessionRequest {
            schema_version: JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                .to_string(),
            session_context: fixture.session_context.clone(),
        };

        let run_candidate_request = RunCandidateRequest {
            schema_version: JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                .to_string(),
            fitted_parameters: fixture.fitted_parameters,
            requested_seed: fixture.simulation_seed,
            requested_output: JointRefinementWorkerRequestedOutput::ExactSummaryOnly,
        };

        let initialize_request_size =
            serialize_joint_refinement_worker_payload(&initialize_request)
                .expect("initialize-session request should serialize")
                .len();

        let run_candidate_request_size =
            serialize_joint_refinement_worker_payload(&run_candidate_request)
                .expect("run-candidate request should serialize")
                .len();

        assert!(
            run_candidate_request_size < initialize_request_size,
            "run-candidate request should stay smaller than initialize-session request",
        );
    }

    fn full_path_seed_summary_fixture(
        fixture: &JointRefinementSummarySimulationFixture,
        session_state: &JointRefinementSummarySimulationSessionState,
    ) -> (
        JointRefinementSeedSummary,
        janiform_engine_simulation::SeededTickSimulationNativeDiagnostics,
    ) {
        let run_result = run_seeded_tick_event_simulation(
            build_seeded_summary_simulation_run_config(
                session_state,
                &fixture.fitted_parameters,
                fixture.simulation_seed,
            )
            .expect("summary simulation run config should build"),
            CancellationToken::uncancelled(),
        )
        .expect("full-path seeded simulation should succeed");

        let simulated_5m_intervals = aggregate_simulated_path_to_interval_template(
            run_result.fine_path.as_slice(),
            fixture
                .session_context
                .simulated_5m_interval_template
                .as_slice(),
            janiform_foundations::time::NANOS_PER_FIVE_MINUTES,
        );

        let simulated_15m_intervals = aggregate_simulated_path_to_interval_template(
            run_result.fine_path.as_slice(),
            fixture
                .session_context
                .simulated_15m_interval_template
                .as_slice(),
            janiform_foundations::time::NANOS_PER_FIFTEEN_MINUTES,
        );

        let simulated_hourly_intervals = aggregate_simulated_path_to_interval_template(
            run_result.fine_path.as_slice(),
            fixture
                .session_context
                .simulated_hourly_interval_template
                .as_slice(),
            NANOS_PER_HOUR,
        );

        let simulated_5m_log_returns = simulated_5m_intervals
            .iter()
            .map(|interval| interval.log_return)
            .collect::<Vec<_>>();

        let simulated_15m_log_returns = simulated_15m_intervals
            .iter()
            .map(|interval| interval.log_return)
            .collect::<Vec<_>>();

        let simulated_1h_log_returns = simulated_hourly_intervals
            .iter()
            .map(|interval| interval.log_return)
            .collect::<Vec<_>>();

        let simulated_1d_intervals = aggregate_intervals_to_fixed_duration(
            simulated_hourly_intervals.as_slice(),
            NANOS_PER_DAY,
        );

        let simulated_1d_log_returns = simulated_1d_intervals
            .iter()
            .map(|interval| interval.log_return)
            .collect::<Vec<_>>();

        let simulated_tick_returns = sampled_trade_returns(
            run_result.fine_path.as_slice(),
            fixture.session_context.observed_tick_shape_target_count,
        );

        (
            JointRefinementSeedSummary {
                simulated_5m_intervals: simulated_5m_intervals.clone(),
                simulated_15m_intervals: simulated_15m_intervals.clone(),
                simulated_tick_shape_returns: scale_normalized_returns(
                    simulated_tick_returns.as_slice(),
                ),
                simulated_5m_shape_returns: scale_normalized_returns(
                    simulated_5m_log_returns.as_slice(),
                ),
                simulated_15m_shape_returns: scale_normalized_returns(
                    simulated_15m_log_returns.as_slice(),
                ),
                simulated_1h_shape_returns: scale_normalized_returns(
                    simulated_1h_log_returns.as_slice(),
                ),
                simulated_1d_shape_returns: scale_normalized_returns(
                    simulated_1d_log_returns.as_slice(),
                ),
                simulated_tick_return_scale_summary: return_scale_summary_from_returns(
                    simulated_tick_returns.as_slice(),
                ),
                simulated_5m_return_scale_summary: return_scale_summary(
                    simulated_5m_intervals.as_slice(),
                ),
                simulated_15m_return_scale_summary: return_scale_summary(
                    simulated_15m_intervals.as_slice(),
                ),
                simulated_1h_return_scale_summary: return_scale_summary(
                    simulated_hourly_intervals.as_slice(),
                ),
                simulated_1d_return_scale_summary: return_scale_summary(
                    simulated_1d_intervals.as_slice(),
                ),
                simulated_5m_business_time_return_scale_summaries:
                    return_scale_summaries_by_business_time_bucket(
                        simulated_5m_intervals.as_slice(),
                        fixture
                            .session_context
                            .observed_5m_business_time_bucket_thresholds,
                    ),
                simulated_15m_business_time_return_scale_summaries:
                    return_scale_summaries_by_business_time_bucket(
                        simulated_15m_intervals.as_slice(),
                        fixture
                            .session_context
                            .observed_15m_business_time_bucket_thresholds,
                    ),
                simulated_tick_intraday_mean_absolute_returns:
                    mean_absolute_trade_return_by_utc_hour(run_result.fine_path.as_slice()),
                simulated_5m_intraday_mean_absolute_returns:
                    crate::stages::joint_metrics::mean_absolute_return_by_utc_hour(
                        simulated_5m_intervals.as_slice(),
                    ),
                simulated_15m_intraday_mean_absolute_returns:
                    crate::stages::joint_metrics::mean_absolute_return_by_utc_hour(
                        simulated_15m_intervals.as_slice(),
                    ),
                simulated_hourly_tick_counts: tick_count_series(
                    simulated_hourly_intervals.as_slice(),
                ),
                simulated_15m_rv_activity_metric: rolling_variance_activity_metric(
                    simulated_15m_intervals.as_slice(),
                ),
                simulated_long_variance_horizon_summaries: long_variance_horizon_summaries(
                    LongVarianceHorizonInputIntervals {
                        five_minute_return_intervals: simulated_5m_intervals.as_slice(),
                        five_minute_activity_intervals: simulated_5m_intervals.as_slice(),
                        hourly_return_intervals: simulated_hourly_intervals.as_slice(),
                        hourly_activity_intervals: simulated_hourly_intervals.as_slice(),
                    },
                ),
            },
            run_result.native_diagnostics(),
        )
    }

    fn joint_refinement_summary_fixture() -> JointRefinementSummarySimulationFixture {
        static FIXTURE: LazyLock<JointRefinementSummarySimulationFixture> = LazyLock::new(|| {
            let calendar_registry = calibration_stage_test_registry();

            let candidate_generation_input = return_components_stage_candidate_generation_fixture();

            let seasonality_payload = seasonality_stage_success_payload();
            let hawkes_payload = hawkes_stage_success_payload();
            let return_components_payload = return_components_stage_success_payload();

            let observed_signal_preparation = prepare_observed_signal_inputs_for_joint_refinement(
                "XNYS",
                &calendar_registry,
                candidate_generation_input.clone(),
                resolve_activity_calibration_context(
                    &janiform_calibration_artifacts::CalibrationStagePayloadSource::InMemoryOutput {
                        payload: hawkes_payload.clone(),
                    },
                ),
            )
            .expect("joint refinement observed signal preparation should succeed");

            let observed_open_event_rate_per_hour =
                selected_hawkes_observed_open_event_rate_per_hour(&hawkes_payload)
                    .expect("selected hawkes observed open-session event rate should resolve");

            let observed_data = build_observed_data(JointRefinementObservedDataBuildInput {
                calendar_registry: &calendar_registry,
                exchange_code: "XNYS",
                return_components_fitted_parameters: &return_components_payload.fitted_parameters,
                observed_signal_preparation: &observed_signal_preparation,
                candidate_generation_input: &candidate_generation_input,
                coarse_data_records: None,
                tick_objective_data_usage_policy:
                    crate::market_data::preparation::CalibrationTickObjectiveDataUsagePolicy::AllAvailable,
                observed_open_event_rate_per_hour,
            })
            .expect("joint refinement observed data should build");

            let runtime_interval_seasonality_factors =
                build_runtime_interval_seasonality_factors_from_components(
                    RuntimeIntervalSeasonalityFactorsBuildInput {
                        calendar_registry: &calendar_registry,
                        exchange_code: "XNYS",
                        clock_model: &seasonality_payload.clock_model,
                        range_start_utc_nanos: observed_data
                            .hourly_simulation_start_utc
                            .timestamp_nanos_opt()
                            .expect("hourly simulation start timestamp should fit in i64"),
                        range_end_utc_nanos: observed_data
                            .hourly_simulation_end_utc
                            .timestamp_nanos_opt()
                            .expect("hourly simulation end timestamp should fit in i64"),
                        interval_duration_nanos: NANOS_PER_HOUR,
                    },
                )
                .expect("runtime interval seasonality factors should build");

            JointRefinementSummarySimulationFixture {
                session_context: JointRefinementSummarySimulationSessionContext {
                    calendar_registry_artifact: build_exchange_calendar_registry_artifact(
                        &calendar_registry,
                        "XNYS",
                        observed_data.hourly_simulation_start_utc,
                    )
                    .expect("calendar registry artifact should build"),
                    exchange_code: "XNYS".to_string(),
                    known_scheduled_event_timestamps_utc_nanos: return_components_payload
                        .fitted_parameters
                        .news_process_configuration
                        .arrivals
                        .known
                        .scheduled_event_timestamps_utc_nanos
                        .clone(),
                    runtime_interval_seasonality_factors,
                    hourly_simulation_start_utc_nanos: observed_data
                        .hourly_simulation_start_utc
                        .timestamp_nanos_opt()
                        .expect("hourly simulation start timestamp should fit in i64"),
                    hourly_simulation_interval_count: observed_data
                        .hourly_simulation_interval_count,
                    surprise_deterministic_intensity_profile: observed_data
                        .surprise_deterministic_intensity_profile
                        .clone(),
                    simulated_5m_interval_template: observed_data
                        .simulated_5m_interval_template
                        .clone(),
                    simulated_15m_interval_template: observed_data
                        .simulated_15m_interval_template
                        .clone(),
                    simulated_hourly_interval_template: observed_data
                        .simulated_hourly_interval_template
                        .clone(),
                    observed_5m_business_time_bucket_thresholds: observed_data
                        .observed_5m_business_time_bucket_thresholds,
                    observed_15m_business_time_bucket_thresholds: observed_data
                        .observed_15m_business_time_bucket_thresholds,
                    observed_tick_shape_target_count: observed_data
                        .observed_tick_shape_returns
                        .len(),
                },
                fitted_parameters: JointRefinementStageFittedParameters {
                    hawkes_fitted_parameters: hawkes_payload.fitted_parameters,
                    return_components_fitted_parameters: return_components_payload
                        .fitted_parameters,
                    pricing_block_fitted_parameters: None,
                },
                simulation_seed: 101,
            }
        });

        FIXTURE.clone()
    }

    pub(crate) fn joint_refinement_summary_session_context_fixture()
    -> JointRefinementSummarySimulationSessionContext {
        joint_refinement_summary_fixture().session_context
    }

    fn spawn_worker_summary_response(
        session_context: &JointRefinementSummarySimulationSessionContext,
        run_candidate_request: &RunCandidateRequest,
    ) -> JointRefinementSummarySimulationWorkerResponse {
        let worker_executable = worker_executable_path();

        let mut child = spawn_worker_process(worker_executable.as_path());

        let (mut stdin, mut stdout) = worker_stdio(&mut child);

        write_joint_refinement_worker_frame(
            &mut stdin,
            &JointRefinementSummarySimulationWorkerRequest::InitializeSession(Box::new(
                InitializeSessionRequest {
                    schema_version:
                        JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                            .to_string(),
                    session_context: session_context.clone(),
                },
            )),
        )
        .expect("initialize-session request should write");

        let initialize_response = read_joint_refinement_worker_frame::<
            JointRefinementSummarySimulationWorkerResponse,
        >(&mut stdout)
        .expect("initialize-session response should deserialize")
        .expect("initialize-session response should exist");

        assert!(matches!(
            initialize_response,
            JointRefinementSummarySimulationWorkerResponse::InitializeSession {
                response,
                ..
            } if matches!(response.as_ref(), InitializeSessionResponse::Success { .. })
        ));

        write_joint_refinement_worker_frame(
            &mut stdin,
            &JointRefinementSummarySimulationWorkerRequest::RunCandidate(Box::new(
                run_candidate_request.clone(),
            )),
        )
        .expect("run-candidate request should write");

        let run_candidate_response = read_joint_refinement_worker_frame::<
            JointRefinementSummarySimulationWorkerResponse,
        >(&mut stdout)
        .expect("run-candidate response should deserialize")
        .expect("run-candidate response should exist");

        write_joint_refinement_worker_frame(
            &mut stdin,
            &JointRefinementSummarySimulationWorkerRequest::CloseSession(Box::new(
                CloseSessionRequest {
                    schema_version:
                        JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                            .to_string(),
                },
            )),
        )
        .expect("close-session request should write");

        let close_response = read_joint_refinement_worker_frame::<
            JointRefinementSummarySimulationWorkerResponse,
        >(&mut stdout)
        .expect("close-session response should deserialize")
        .expect("close-session response should exist");

        assert!(matches!(
            close_response,
            JointRefinementSummarySimulationWorkerResponse::CloseSession { .. }
        ));

        run_candidate_response
    }

    fn spawn_worker_process(worker_executable: &Path) -> Child {
        Command::new(worker_executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("worker executable should spawn")
    }

    fn worker_stdio(
        child: &mut Child,
    ) -> (
        std::process::ChildStdin,
        std::io::BufReader<std::process::ChildStdout>,
    ) {
        let stdin = child
            .stdin
            .take()
            .expect("worker stdin should be available");

        let stdout = child
            .stdout
            .take()
            .expect("worker stdout should be available");

        (stdin, std::io::BufReader::new(stdout))
    }

    fn worker_executable_path() -> PathBuf {
        static WORKER_EXECUTABLE: LazyLock<PathBuf> = LazyLock::new(|| {
            let manifest_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

            let workspace_root = manifest_directory
                .parent()
                .and_then(Path::parent)
                .expect("workspace root should resolve from manifest directory");

            let current_executable =
                std::env::current_exe().expect("current test executable should resolve");

            let profile_directory = current_executable
                .parent()
                .and_then(Path::parent)
                .expect("cargo profile directory should resolve");

            let worker_executable = profile_directory.join(if cfg!(windows) {
                "janiform-joint-refinement-worker.exe"
            } else {
                "janiform-joint-refinement-worker"
            });

            if worker_executable_requires_rebuild(worker_executable.as_path(), workspace_root) {
                let cargo_path = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());

                let build_status = Command::new(cargo_path)
                    .current_dir(workspace_root)
                    .args(["build", "-p", "janiform-joint-refinement-worker"])
                    .status()
                    .expect("worker crate build should launch");

                assert!(
                    build_status.success(),
                    "worker crate build failed with status {build_status}",
                );
            }

            assert!(
                worker_executable.is_file(),
                "worker executable should exist at {}",
                worker_executable.display(),
            );

            worker_executable
        });

        WORKER_EXECUTABLE.clone()
    }

    fn worker_executable_requires_rebuild(worker_executable: &Path, workspace_root: &Path) -> bool {
        if !worker_executable.is_file() {
            return true;
        }

        let executable_modified =
            match fs::metadata(worker_executable).and_then(|metadata| metadata.modified()) {
                Ok(modified) => modified,
                Err(_) => return true,
            };

        latest_worker_source_modified_at(workspace_root)
            .is_none_or(|source_modified| source_modified > executable_modified)
    }

    fn latest_worker_source_modified_at(workspace_root: &Path) -> Option<SystemTime> {
        [
            workspace_root.join("Cargo.toml"),
            workspace_root.join("Cargo.lock"),
            workspace_root.join("crates/janiform-joint-refinement-worker/Cargo.toml"),
            workspace_root.join("crates/janiform-joint-refinement-worker/src"),
            workspace_root.join("crates/janiform-calibration-artifacts/Cargo.toml"),
            workspace_root.join("crates/janiform-calibration-artifacts/src"),
            workspace_root.join("crates/janiform-calibration-runtime/Cargo.toml"),
            workspace_root.join("crates/janiform-calibration-runtime/src/stages/joint_refinement"),
            workspace_root.join("crates/janiform-engine-simulation/Cargo.toml"),
            workspace_root.join("crates/janiform-engine-simulation/src"),
            workspace_root.join("crates/janiform-model-kernels/Cargo.toml"),
            workspace_root.join("crates/janiform-model-kernels/src"),
        ]
        .into_iter()
        .filter_map(|path| latest_path_modified_at(path.as_path()))
        .max()
    }

    fn latest_path_modified_at(path: &Path) -> Option<SystemTime> {
        let metadata = fs::metadata(path).ok()?;

        if metadata.is_file() {
            return metadata.modified().ok();
        }

        if !metadata.is_dir() {
            return None;
        }

        let mut latest_modified = metadata.modified().ok();

        let entries = fs::read_dir(path).ok()?;

        for entry in entries.flatten() {
            if let Some(entry_modified) = latest_path_modified_at(entry.path().as_path()) {
                latest_modified = Some(latest_modified.map_or(entry_modified, |current_latest| {
                    current_latest.max(entry_modified)
                }));
            }
        }

        latest_modified
    }

    fn seasonality_stage_success_payload() -> SeasonalityStageNormalizedPayload {
        static PAYLOAD: LazyLock<SeasonalityStageNormalizedPayload> = LazyLock::new(|| {
            let report = run_seasonality_calibration_stage(
                &build_seasonality_stage_run_spec(CalibrationExecutionMode::Memory),
                &calibration_stage_test_registry(),
                SeasonalityStageExecutionInput::from_timestamp_event_records(
                    calibration_stage_timestamp_fixture(),
                ),
            );

            assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

            report
                .normalized_stage_payload
                .expect("seasonality stage fixture should produce normalized payload")
        });

        PAYLOAD.clone()
    }

    fn hawkes_stage_success_payload() -> HawkesStageFittedPayload {
        static PAYLOAD: LazyLock<HawkesStageFittedPayload> = LazyLock::new(|| {
            let report = run_hawkes_calibration_stage(
                &build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory),
                &calibration_stage_test_registry(),
                HawkesStageExecutionInput::from_in_memory_seasonality_output(
                    seasonality_stage_success_payload(),
                    hawkes_stage_candidate_fixture(),
                ),
            );

            assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

            report
                .fitted_stage_payload
                .expect("hawkes stage fixture should produce fitted payload")
        });

        PAYLOAD.clone()
    }

    fn return_components_stage_success_payload() -> ReturnComponentsStageFittedPayload {
        static PAYLOAD: LazyLock<ReturnComponentsStageFittedPayload> = LazyLock::new(|| {
            let report = run_return_components_calibration_stage(
                &build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory),
                &calibration_stage_test_registry(),
                ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                    hawkes_stage_success_payload(),
                    return_components_stage_candidate_generation_fixture(),
                ),
            );

            assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

            report
                .fitted_stage_payload
                .expect("return-components stage fixture should produce fitted payload")
        });

        PAYLOAD.clone()
    }

    fn build_data_inputs() -> CalibrationRunDataInputs {
        CalibrationRunDataInputs::try_new(
            "XNYS",
            "dataset://synthetic/trades/v1",
            None,
            Some("dataset://synthetic/options/v1".to_string()),
        )
        .expect("data inputs should validate")
    }

    fn build_seed_settings() -> CalibrationRunSeedSettings {
        CalibrationRunSeedSettings::try_new(
            17,
            vec![
                CalibrationStageSeedOverride {
                    stage: CalibrationStage::Seasonality,
                    seed: 101,
                },
                CalibrationStageSeedOverride {
                    stage: CalibrationStage::Hawkes,
                    seed: 202,
                },
            ],
        )
        .expect("seed settings should validate")
    }

    fn build_solver_settings() -> CalibrationOptimizerSettings {
        CalibrationOptimizerSettings::try_new("lbfgs", 64, 1, 1e-8, 1e-6, 10.0)
            .expect("solver settings should validate")
    }

    fn build_mode_aware_stage_handoff(
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
        execution_mode: CalibrationExecutionMode,
        persisted_artifact_reference: &str,
    ) -> CalibrationStageHandoff {
        match execution_mode {
            CalibrationExecutionMode::Memory => {
                CalibrationStageHandoff::in_memory_output(from_stage, to_stage)
            }
            CalibrationExecutionMode::Disk => {
                CalibrationStageHandoff::persisted_artifact_reference(
                    from_stage,
                    to_stage,
                    persisted_artifact_reference,
                )
                .expect("disk-mode persisted handoff should validate")
            }
        }
    }

    fn build_seasonality_stage_run_spec(
        execution_mode: CalibrationExecutionMode,
    ) -> CalibrationRunSpec {
        CalibrationRunSpec::builder(
            "run-summary-simulation-seasonality",
            build_data_inputs(),
            execution_mode,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            vec![
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Seasonality,
                    optimizer_settings: build_solver_settings(),
                },
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Hawkes,
                    optimizer_settings: build_solver_settings(),
                },
            ],
        )
        .with_stage_handoffs(vec![build_mode_aware_stage_handoff(
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            execution_mode,
            "artifact://seasonality/run-summary-simulation",
        )])
        .build()
        .expect("seasonality stage run spec should validate")
    }

    fn build_hawkes_stage_run_spec(execution_mode: CalibrationExecutionMode) -> CalibrationRunSpec {
        CalibrationRunSpec::builder(
            "run-summary-simulation-hawkes",
            build_data_inputs(),
            execution_mode,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            ],
            build_seed_settings(),
            vec![
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Seasonality,
                    optimizer_settings: build_solver_settings(),
                },
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Hawkes,
                    optimizer_settings: build_solver_settings(),
                },
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::ReturnComponents,
                    optimizer_settings: build_solver_settings(),
                },
            ],
        )
        .with_stage_handoffs(vec![
            build_mode_aware_stage_handoff(
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                execution_mode,
                "artifact://seasonality/run-summary-simulation",
            ),
            build_mode_aware_stage_handoff(
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
                execution_mode,
                "artifact://hawkes/run-summary-simulation",
            ),
        ])
        .build()
        .expect("hawkes stage run spec should validate")
    }

    fn build_return_components_stage_pipeline_run_spec(
        execution_mode: CalibrationExecutionMode,
    ) -> CalibrationRunSpec {
        build_hawkes_stage_run_spec(execution_mode)
    }

    fn calibration_stage_test_registry() -> ExchangeCalendarRegistry {
        ExchangeCalendarRegistryLoader::load_from_json(
            r#"
{
  "schema_version": "calendar.registry.v1",
  "artifact_date": "2026-01-31",
  "exchanges": [
    {
      "schema_version": "calendar.v1",
      "exchange_code": "XNYS",
      "exchange_name": "New York Stock Exchange",
      "timezone_name": "America/New_York",
      "rule_metadata": {
        "holiday_calendar_code": "US_EQUITIES",
        "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
      },
      "regular_sessions": [
        {"weekday": "Monday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Tuesday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Wednesday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]}
      ],
      "holiday_dates": [],
      "early_close_sessions": []
    }
  ]
}
"#,
        )
        .expect("calendar registry fixture should load")
    }

    fn calibration_stage_timestamp_fixture() -> Vec<TimestampEventInputRecord> {
        vec![
            timestamp_input_record(0, 2026, 2, 2, 14, 30, 0),
            timestamp_input_record(1, 2026, 2, 2, 15, 30, 0),
            timestamp_input_record(2, 2026, 2, 3, 14, 30, 0),
            timestamp_input_record(3, 2026, 2, 3, 18, 45, 0),
            timestamp_input_record(4, 2026, 2, 4, 14, 30, 0),
            timestamp_input_record(5, 2026, 2, 4, 20, 30, 0),
        ]
    }

    fn timestamp_input_record(
        source_record_position: u64,
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> TimestampEventInputRecord {
        TimestampEventInputRecord {
            source_record_position,
            timestamp_utc: DateTime::from_timestamp_nanos(utc_nanos(
                year, month, day, hour, minute, second,
            )),
        }
    }

    fn hawkes_stage_candidate_fixture() -> Vec<HawkesStageCandidateInput> {
        let source_regime = HawkesStageObservedSourceRegime {
            observation_duration_hours: 12.0,
            open_session_duration_hours: 8.0,
            total_tick_count: 95,
            open_session_tick_count: 84,
            closed_session_tick_count: 11,
            open_session_buy_tick_count: 39,
            open_session_sell_tick_count: 45,
            open_session_mean_interarrival_seconds: 34.0,
            open_session_interarrival_coefficient_of_variation: 1.18,
            open_session_short_gap_threshold_seconds: 17.0,
            open_session_short_gap_ratio: 0.43,
            known_news_open_event_count: 2,
            open_session_known_news_activity_share: 0.06,
        };

        let directional_label_diagnostics = HawkesStageDirectionalLabelDiagnostics {
            policy_id: "tick_price_change_with_flat_move_carryover.v1".to_string(),
            first_inferred_trade_sign: "buy".to_string(),
            provided_label_count: 10,
            inferred_label_count: 74,
            flat_move_carry_count: 9,
            price_up_inference_count: 35,
            price_down_inference_count: 30,
        };

        vec![
            HawkesStageCandidateInput {
                candidate_id: "candidate_high_loss".to_string(),
                lambda_bar: 7.20,
                beta: 0.95,
                structured_alpha: HawkesStructuredAlphaParameterization::new(
                    0.28, 0.10, 0.08, 0.06,
                ),
                structured_rho: HawkesStructuredRhoParameterization::new(0.14, 0.04, 0.03, 0.02),
                self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(1.0)),
                stability_numeric_tolerance: 0.0,
                source_regime: source_regime.clone(),
                directional_label_diagnostics: directional_label_diagnostics.clone(),
            },
            HawkesStageCandidateInput {
                candidate_id: "candidate_stable_low_loss".to_string(),
                lambda_bar: 9.40,
                beta: 20.0,
                structured_alpha: HawkesStructuredAlphaParameterization::new(
                    0.01, 0.002, 0.001, 0.0005,
                ),
                structured_rho: HawkesStructuredRhoParameterization::new(0.08, 0.02, 0.01, 0.01),
                self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(1.0)),
                stability_numeric_tolerance: 0.0,
                source_regime,
                directional_label_diagnostics,
            },
        ]
    }

    fn return_components_stage_candidate_generation_fixture()
    -> ReturnComponentsStageCandidateGenerationInput {
        let source_window = ReturnComponentsStageSignalSourceWindow {
            window_start_utc_nanos: utc_nanos(2026, 2, 2, 13, 0, 0),
            window_end_utc_nanos: utc_nanos(2026, 2, 5, 0, 0, 0),
        };

        ReturnComponentsStageCandidateGenerationInput {
            known_news_calendar: ReturnComponentsKnownNewsCalendarInput {
                source_window,
                scheduled_event_timestamps_utc_nanos: vec![
                    utc_nanos(2026, 2, 2, 14, 30, 0),
                    utc_nanos(2026, 2, 3, 15, 0, 0),
                    utc_nanos(2026, 2, 4, 16, 0, 0),
                ],
            },
            realized_known_news_return_samples: vec![
                ReturnComponentsRealizedKnownNewsReturnSample {
                    event_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 30, 0),
                    attributed_open_return_timestamp_utc_nanos: Some(utc_nanos(
                        2026, 2, 2, 15, 0, 0,
                    )),
                    open_window_jump_return: 0.016,
                    closed_window_gap_return: -0.009,
                    post_shock_activity_lift: 1.20,
                },
                ReturnComponentsRealizedKnownNewsReturnSample {
                    event_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 15, 0, 0),
                    attributed_open_return_timestamp_utc_nanos: Some(utc_nanos(
                        2026, 2, 3, 18, 20, 0,
                    )),
                    open_window_jump_return: -0.012,
                    closed_window_gap_return: 0.004,
                    post_shock_activity_lift: 1.05,
                },
                ReturnComponentsRealizedKnownNewsReturnSample {
                    event_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 16, 0, 0),
                    attributed_open_return_timestamp_utc_nanos: Some(utc_nanos(
                        2026, 2, 4, 19, 15, 0,
                    )),
                    open_window_jump_return: 0.011,
                    closed_window_gap_return: -0.006,
                    post_shock_activity_lift: 0.98,
                },
            ],
            observed_open_session_returns: vec![
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 15, 0, 0),
                    elapsed_same_session_hours: 1.0,
                    log_return: 0.004,
                    post_shock_activity_lift: 1.00,
                    sell_share_proxy: 0.48,
                    source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.48),
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 16, 5, 0),
                    elapsed_same_session_hours: 1.0,
                    log_return: 0.018,
                    post_shock_activity_lift: 1.35,
                    sell_share_proxy: 0.62,
                    source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.62),
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 18, 20, 0),
                    elapsed_same_session_hours: 1.0,
                    log_return: -0.014,
                    post_shock_activity_lift: 1.10,
                    sell_share_proxy: 0.58,
                    source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.58),
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 19, 15, 0),
                    elapsed_same_session_hours: 1.0,
                    log_return: 0.010,
                    post_shock_activity_lift: 0.95,
                    sell_share_proxy: 0.41,
                    source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.41),
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 20, 0, 0),
                    elapsed_same_session_hours: 1.0,
                    log_return: -0.003,
                    post_shock_activity_lift: 0.92,
                    sell_share_proxy: 0.55,
                    source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.55),
                },
            ],
            detected_intraday_jump_candidates: vec![
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 16, 5, 0),
                    jump_return: 0.018,
                    post_shock_activity_lift: 1.35,
                },
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 18, 20, 0),
                    jump_return: -0.014,
                    post_shock_activity_lift: 1.10,
                },
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 19, 15, 0),
                    jump_return: 0.010,
                    post_shock_activity_lift: 0.95,
                },
            ],
            observed_close_open_boundary_observations: vec![
                ReturnComponentsObservedCloseOpenBoundaryObservation {
                    close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 21, 0, 0),
                    close_price: 100.0,
                    open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
                    open_price: 98.70858118310978,
                    post_shock_activity_lift: 1.40,
                },
                ReturnComponentsObservedCloseOpenBoundaryObservation {
                    close_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 21, 0, 0),
                    close_price: 100.0,
                    open_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 14, 30, 0),
                    open_price: 100.80320855069357,
                    post_shock_activity_lift: 1.05,
                },
            ],
            close_open_gap_detection_policy: ReturnComponentsCloseOpenGapDetectionPolicy::new(
                0.005,
            ),
            open_session_sell_share_proxy: 0.52,
            shared_mark_law_template: SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.8)
                    .expect("gamma severity mark configuration should validate"),
            },
            return_clock_exposure_parameters:
                janiform_model_kernels::return_clock::ReturnClockExposureParameters::neutral(),
            tick_grid_observation_model: janiform_calibration_artifacts::TickGridObservationModel {
                tick_size: None,
                tick_size_source: "test".to_string(),
                inference_policy: "test".to_string(),
                observation_policy: "test".to_string(),
            },
            timestamp_batching_observation_model:
                janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModel::disabled(
                ),
            objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights::new(
                1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
            ),
        }
    }
}
