//! Simulation-side hourly factor loading through the shared provider interface.

use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::calendar::ExchangeCalendarRegistry;
use crate::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesIntensityProjection, HawkesKernelParameters,
    HawkesSeasonalityIntervalError, HawkesTransitionError, HawkesTransitionStep,
};
use crate::hourly_factor_provider::{
    HourlyFactorProviderError, HourlyFactorProviderRequest, provide_hourly_factor_contract,
};
use crate::hourly_factors::HourlyFactorContractOutput;
use crate::randomness::{
    SIMULATION_RANDOMNESS_STREAM_POLICY, SIMULATION_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
    SimulationRandomnessFactory, SimulationRandomnessStream, SimulationRandomnessSubstreamDomain,
    SplitMix64SimulationRandomnessFactory,
};

pub const SEEDED_TICK_EVENT_RUN_CONFIG_SCHEMA_VERSION: &str =
    "simulation.seeded_tick_event.run_config.v1";
pub const SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION: &str = "simulation.seeded_tick_event.path.v1";
pub const SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION: &str = "simulation.seeded_tick_event.record.v1";
pub const SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION: &str =
    "simulation.seeded_tick_event.tape_metadata.v1";
pub const SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION: &str =
    "simulation.seeded_tick_event.state_snapshot.v1";
pub const SEEDED_TICK_EVENT_DETERMINISM_POLICY: &str =
    "stable_seed_and_run_config_with_ordered_event_path.v1";

/// Ordering policy for all emitted stochastic updates and recorded buy/sell arrivals.
///
/// Tie-break sequence:
/// 1. lower timestamp first
/// 2. endogenous events before exogenous news
/// 3. buy before sell when endogenous timestamps tie
/// 4. generation order fallback for exact key collisions
pub const SEEDED_TICK_EVENT_ORDERING_POLICY: &str =
    "timestamp_then_endogenous_before_exogenous_then_buy_before_sell_then_generation_order.v1";
pub const SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY: &str =
    "event_time_timestamp_utc_nanos_without_bucket_identifier.v1";
pub const SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY: &str = "lowercase_buy_or_sell.v1";
pub const SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY: &str =
    "post_event_transition_pre_following_same_timestamp_updates.v1";
pub const SEEDED_TICK_EVENT_INTERVAL_BOUNDARY_POLICY: &str =
    "hourly_factor_start_inclusive_end_exclusive_with_strict_consecutive_coverage.v1";
pub const SEEDED_TICK_EVENT_STATE_CARRYOVER_POLICY: &str =
    "full_4d_hawkes_excitation_state_carryover_across_interval_boundaries.v1";
pub const SEEDED_TICK_EVENT_FACTOR_CONTROL_POLICY: &str =
    "interval_aligned_s_h_nu_h_control_intensity_only_without_intra_interval_grid.v1";
pub const SEEDED_TICK_EVENT_INTRA_INTERVAL_SCHEDULING_POLICY: &str =
    "event_time_updates_with_sequential_hawkes_transition_composition.v1";

pub const SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY: &str = SIMULATION_RANDOMNESS_STREAM_POLICY;
pub const SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY: &str =
    SIMULATION_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY;

const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;
const MAX_STOCHASTIC_UPDATES_PER_INTERVAL: u64 = 1_000_000;

/// State snapshot capture mode for per-event diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeededTickEventStateSnapshotCapture {
    Disabled,
    PostEventTransition,
}

impl SeededTickEventStateSnapshotCapture {
    const fn capture_point_policy(self) -> Option<&'static str> {
        match self {
            Self::Disabled => None,
            Self::PostEventTransition => {
                Some(SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY)
            }
        }
    }
}

impl Default for SeededTickEventStateSnapshotCapture {
    fn default() -> Self {
        Self::Disabled
    }
}

/// Optional run settings for seeded tick-event simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeededTickSimulationRunOptions {
    /// Enable optional debug trace emission in future simulator stages.
    pub debug_trace_enabled: bool,

    /// Control optional Hawkes state snapshot capture on each event tape record.
    pub state_snapshot_capture: SeededTickEventStateSnapshotCapture,
}

impl Default for SeededTickSimulationRunOptions {
    fn default() -> Self {
        Self {
            debug_trace_enabled: false,
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }
    }
}

/// Required model inputs for seeded tick-event simulation runs.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededTickSimulationModelInputs {
    /// Canonical Hawkes kernel parameter payload.
    pub kernel_parameters: HawkesKernelParameters,

    /// Excitation state carried into the first simulated interval.
    pub initial_excitation_state: HawkesExcitationState,

    /// Interval-aligned hourly factors (`s_h`, `nu_h`) used by the simulator.
    pub hourly_factor_contract: HourlyFactorContractOutput,
}

/// Typed public run contract for deterministic seeded tick-event simulation.
///
/// Required fields:
/// - `seed`
/// - `start_time_utc`
/// - `horizon_interval_count`
/// - `model_inputs`
///
/// Optional fields:
/// - `options`, defaults to `SeededTickSimulationRunOptions::default()`
#[derive(Debug, Clone, PartialEq)]
pub struct SeededTickSimulationRunConfig {
    pub seed: u64,
    pub start_time_utc: DateTime<Utc>,
    pub horizon_interval_count: usize,
    pub model_inputs: SeededTickSimulationModelInputs,
    pub options: SeededTickSimulationRunOptions,
}

impl SeededTickSimulationRunConfig {
    /// Build and validate one seeded run configuration payload.
    ///
    /// Failure modes:
    /// - invalid horizon value (`horizon_interval_count == 0`)
    /// - missing or incomplete hourly factor interval vectors
    /// - mismatched hourly factor diagnostics interval count
    /// - non-consecutive or invalid hourly factor interval bounds in simulated horizon
    /// - start time not aligned to first hourly factor interval start
    /// - requested horizon exceeds available factor intervals
    pub fn try_new(
        seed: u64,
        start_time_utc: DateTime<Utc>,
        horizon_interval_count: usize,
        model_inputs: SeededTickSimulationModelInputs,
        options: Option<SeededTickSimulationRunOptions>,
    ) -> Result<Self, SeededTickSimulationRunError> {
        let run_config = Self {
            seed,
            start_time_utc,
            horizon_interval_count,
            model_inputs,
            options: options.unwrap_or_default(),
        };

        run_config.validate()?;

        Ok(run_config)
    }

    pub fn validate(&self) -> Result<(), SeededTickSimulationRunError> {
        if self.horizon_interval_count == 0 {
            return Err(SeededTickSimulationRunError::InvalidHorizonIntervalCount { value: 0 });
        }

        let hourly_factor_contract = &self.model_inputs.hourly_factor_contract;
        let expected_interval_count = hourly_factor_contract.interval_indices.len();
        if expected_interval_count == 0 {
            return Err(SeededTickSimulationRunError::MissingHourlyFactorIntervals);
        }

        if hourly_factor_contract.diagnostics.interval_count != expected_interval_count {
            return Err(
                SeededTickSimulationRunError::HourlyFactorDiagnosticsIntervalCountMismatch {
                    expected_interval_count,
                    diagnostics_interval_count: hourly_factor_contract.diagnostics.interval_count,
                },
            );
        }

        validate_hourly_factor_field_length(
            "interval_start_utc_nanos",
            expected_interval_count,
            hourly_factor_contract.interval_start_utc_nanos.len(),
        )?;
        validate_hourly_factor_field_length(
            "interval_end_utc_nanos",
            expected_interval_count,
            hourly_factor_contract.interval_end_utc_nanos.len(),
        )?;
        validate_hourly_factor_field_length(
            "s_h",
            expected_interval_count,
            hourly_factor_contract.s_h.len(),
        )?;
        validate_hourly_factor_field_length(
            "nu_h",
            expected_interval_count,
            hourly_factor_contract.nu_h.len(),
        )?;

        if self.horizon_interval_count > expected_interval_count {
            return Err(
                SeededTickSimulationRunError::HorizonExceedsAvailableIntervals {
                    requested_interval_count: self.horizon_interval_count,
                    available_interval_count: expected_interval_count,
                },
            );
        }

        let start_time_utc_nanos = self.start_time_utc.timestamp_nanos_opt().ok_or(
            SeededTickSimulationRunError::StartTimeOutOfNanosecondRange {
                value: self.start_time_utc,
            },
        )?;
        let first_interval_start_utc_nanos = hourly_factor_contract.interval_start_utc_nanos[0];

        if start_time_utc_nanos != first_interval_start_utc_nanos {
            return Err(
                SeededTickSimulationRunError::StartTimeDoesNotMatchHourlyFactorCoverage {
                    start_time_utc_nanos,
                    first_interval_start_utc_nanos,
                },
            );
        }

        validate_hourly_factor_interval_coverage(
            &hourly_factor_contract.interval_start_utc_nanos,
            &hourly_factor_contract.interval_end_utc_nanos,
            self.horizon_interval_count,
        )?;

        Ok(())
    }
}

/// Side label for one simulated buy or sell event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeededTickEventSide {
    Buy,
    Sell,
}

/// Optional Hawkes state snapshot attached to one event tape record.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SeededTickEventStateSnapshot {
    pub y_plus_plus: f64,
    pub y_plus_minus: f64,
    pub y_minus_plus: f64,
    pub y_minus_minus: f64,
    pub deseasonalized_lambda_plus: f64,
    pub deseasonalized_lambda_minus: f64,
}

impl SeededTickEventStateSnapshot {
    const fn from_state(state: HawkesExcitationState, lambda_bar: f64) -> Self {
        let deseasonalized_intensities = state.project_deseasonalized_intensities(lambda_bar);

        Self {
            y_plus_plus: state.y_plus_plus,
            y_plus_minus: state.y_plus_minus,
            y_minus_plus: state.y_minus_plus,
            y_minus_minus: state.y_minus_minus,
            deseasonalized_lambda_plus: deseasonalized_intensities.lambda_plus,
            deseasonalized_lambda_minus: deseasonalized_intensities.lambda_minus,
        }
    }
}

/// One deterministic event-path record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededTickEventRecord {
    pub event_index: u64,
    pub timestamp_utc_nanos: i64,
    pub side: SeededTickEventSide,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_snapshot: Option<SeededTickEventStateSnapshot>,
}

/// Machine-readable event tape metadata for deterministic serialization and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SeededTickEventTapeMetadata {
    pub schema_version: &'static str,
    pub event_record_schema_version: &'static str,
    pub state_snapshot_schema_version: &'static str,
    pub event_ordering_policy: &'static str,
    pub timestamp_representation_policy: &'static str,
    pub side_encoding_policy: &'static str,
    pub state_snapshot_capture: SeededTickEventStateSnapshotCapture,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_snapshot_capture_point_policy: Option<&'static str>,
}

/// Canonical serializable event tape payload for deterministic run outputs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededTickEventTapeOutput<'a> {
    pub schema_version: &'static str,
    pub metadata: SeededTickEventTapeMetadata,
    pub event_tape: &'a [SeededTickEventRecord],
}

/// Deterministic seeded event-path output object.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededTickSimulationRunResult {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub event_ordering_policy: &'static str,
    pub randomness_stream_policy: &'static str,
    pub randomness_substream_derivation_policy: &'static str,
    pub seed: u64,
    pub start_time_utc: DateTime<Utc>,
    pub end_time_utc: DateTime<Utc>,
    pub simulated_interval_count: usize,
    pub event_tape_metadata: SeededTickEventTapeMetadata,
    pub event_path: Vec<SeededTickEventRecord>,
    pub final_excitation_state: HawkesExcitationState,
    pub debug_trace_enabled: bool,
}

impl SeededTickSimulationRunResult {
    /// Build the canonical machine-readable event tape payload.
    pub fn event_tape_output(&self) -> SeededTickEventTapeOutput<'_> {
        SeededTickEventTapeOutput {
            schema_version: self.schema_version,
            metadata: self.event_tape_metadata,
            event_tape: &self.event_path,
        }
    }

    /// Serialize the canonical event tape payload to deterministic JSON.
    pub fn serialize_event_tape_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.event_tape_output())
    }
}

/// Structured simulator run failure with explicit validation diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub enum SeededTickSimulationRunError {
    InvalidHorizonIntervalCount {
        value: usize,
    },
    MissingHourlyFactorIntervals,
    HourlyFactorFieldLengthMismatch {
        field_name: &'static str,
        expected_interval_count: usize,
        observed_interval_count: usize,
    },
    HourlyFactorDiagnosticsIntervalCountMismatch {
        expected_interval_count: usize,
        diagnostics_interval_count: usize,
    },
    HorizonExceedsAvailableIntervals {
        requested_interval_count: usize,
        available_interval_count: usize,
    },
    StartTimeOutOfNanosecondRange {
        value: DateTime<Utc>,
    },
    StartTimeDoesNotMatchHourlyFactorCoverage {
        start_time_utc_nanos: i64,
        first_interval_start_utc_nanos: i64,
    },
    InvalidHourlyFactorIntervalBounds {
        interval_index: usize,
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    },
    NonConsecutiveHourlyFactorCoverage {
        previous_interval_index: usize,
        previous_interval_end_utc_nanos: i64,
        current_interval_index: usize,
        current_interval_start_utc_nanos: i64,
    },
    InvalidGeneratedNewsSeverityMark {
        value: f64,
    },
    InvalidTransitionStep {
        source: HawkesTransitionError,
    },
}

impl Display for SeededTickSimulationRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHorizonIntervalCount { value } => write!(
                formatter,
                "horizon_interval_count must be greater than 0, found {value}"
            ),
            Self::MissingHourlyFactorIntervals => write!(
                formatter,
                "hourly_factor_contract.interval_count must be greater than 0"
            ),
            Self::HourlyFactorFieldLengthMismatch {
                field_name,
                expected_interval_count,
                observed_interval_count,
            } => write!(
                formatter,
                "hourly_factor_contract.{field_name} length must match interval_count ({expected_interval_count}), found {observed_interval_count}"
            ),
            Self::HourlyFactorDiagnosticsIntervalCountMismatch {
                expected_interval_count,
                diagnostics_interval_count,
            } => write!(
                formatter,
                "hourly_factor_contract.diagnostics.interval_count must match interval_count ({expected_interval_count}), found {diagnostics_interval_count}"
            ),
            Self::HorizonExceedsAvailableIntervals {
                requested_interval_count,
                available_interval_count,
            } => write!(
                formatter,
                "horizon_interval_count ({requested_interval_count}) exceeds hourly factor interval_count ({available_interval_count})"
            ),
            Self::StartTimeOutOfNanosecondRange { value } => write!(
                formatter,
                "start_time_utc cannot be represented in nanoseconds, found {value}"
            ),
            Self::StartTimeDoesNotMatchHourlyFactorCoverage {
                start_time_utc_nanos,
                first_interval_start_utc_nanos,
            } => write!(
                formatter,
                "start_time_utc nanos ({start_time_utc_nanos}) must match hourly factor first interval start ({first_interval_start_utc_nanos})"
            ),
            Self::InvalidHourlyFactorIntervalBounds {
                interval_index,
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            } => write!(
                formatter,
                "hourly_factor_contract interval {interval_index} has invalid bounds, start={interval_start_utc_nanos}, end={interval_end_utc_nanos}"
            ),
            Self::NonConsecutiveHourlyFactorCoverage {
                previous_interval_index,
                previous_interval_end_utc_nanos,
                current_interval_index,
                current_interval_start_utc_nanos,
            } => write!(
                formatter,
                "hourly_factor_contract interval coverage must be consecutive, interval {previous_interval_index} end ({previous_interval_end_utc_nanos}) must equal interval {current_interval_index} start ({current_interval_start_utc_nanos})"
            ),
            Self::InvalidGeneratedNewsSeverityMark { value } => write!(
                formatter,
                "generated exogenous news severity mark must be finite and non-negative, found {value}"
            ),
            Self::InvalidTransitionStep { source } => {
                write!(
                    formatter,
                    "failed to apply canonical Hawkes transition step: {source}"
                )
            }
        }
    }
}

impl Error for SeededTickSimulationRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidTransitionStep { source } => Some(source),
            _ => None,
        }
    }
}

/// Run deterministic seeded tick-event simulation using the default splitmix64 randomness policy.
pub fn run_seeded_tick_event_simulation(
    run_config: SeededTickSimulationRunConfig,
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;
    run_seeded_tick_event_simulation_with_randomness_factory(run_config, &randomness_factory)
}

/// Run deterministic seeded tick-event simulation with an injected randomness factory.
///
/// This hook keeps randomness centralized in one abstraction while enabling fixed fixture
/// streams for deterministic testing and reference-sequence generation.
pub fn run_seeded_tick_event_simulation_with_randomness_factory<F>(
    run_config: SeededTickSimulationRunConfig,
    randomness_factory: &F,
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    run_config.validate()?;

    let state_snapshot_capture = run_config.options.state_snapshot_capture;
    let lambda_bar = run_config.model_inputs.kernel_parameters.lambda_bar;
    let event_tape_metadata = build_seeded_tick_event_tape_metadata(state_snapshot_capture);
    let mut event_path: Vec<SeededTickEventRecord> = Vec::new();
    let mut next_event_index: u64 = 0;
    let mut next_state = run_config.model_inputs.initial_excitation_state;

    for interval_index in 0..run_config.horizon_interval_count {
        let interval_start_utc_nanos = run_config
            .model_inputs
            .hourly_factor_contract
            .interval_start_utc_nanos[interval_index];
        let interval_end_utc_nanos = run_config
            .model_inputs
            .hourly_factor_contract
            .interval_end_utc_nanos[interval_index];

        if interval_end_utc_nanos <= interval_start_utc_nanos {
            return Err(
                SeededTickSimulationRunError::InvalidHourlyFactorIntervalBounds {
                    interval_index,
                    interval_start_utc_nanos,
                    interval_end_utc_nanos,
                },
            );
        }

        let interval_duration_nanos = interval_end_utc_nanos - interval_start_utc_nanos;
        let interval_duration_in_hours = interval_duration_nanos as f64 / ONE_HOUR_NANOS as f64;
        let interval_projection = next_state.project_intensities(
            run_config.model_inputs.kernel_parameters.lambda_bar,
            run_config.model_inputs.hourly_factor_contract.s_h[interval_index],
        );
        let interval_news_intensity =
            run_config.model_inputs.hourly_factor_contract.nu_h[interval_index].max(0.0);
        let interval_updates = schedule_interval_stochastic_updates(
            run_config.seed,
            interval_index,
            interval_start_utc_nanos,
            interval_duration_nanos,
            interval_duration_in_hours,
            interval_projection,
            interval_news_intensity,
            randomness_factory,
        );

        let mut previous_timestamp_utc_nanos = interval_start_utc_nanos;
        for interval_update in interval_updates {
            // Intra-interval updates are processed in event time and each update composes
            // one canonical transition step onto the carried Hawkes state.
            let elapsed_hours_since_previous_update = derive_elapsed_hours(
                previous_timestamp_utc_nanos,
                interval_update.timestamp_utc_nanos(),
            );
            let transition_step =
                interval_update.transition_step(elapsed_hours_since_previous_update);

            next_state = evolve_hawkes_state_for_simulation_step(
                next_state,
                run_config.model_inputs.kernel_parameters,
                transition_step,
            )
            .map_err(map_transition_error_to_seeded_simulation_run_error)?;

            match interval_update {
                IntervalStochasticUpdate::EndogenousEvent {
                    timestamp_utc_nanos,
                    side,
                    ..
                } => {
                    event_path.push(SeededTickEventRecord {
                        event_index: next_event_index,
                        timestamp_utc_nanos,
                        side,
                        state_snapshot: build_event_state_snapshot(
                            next_state,
                            lambda_bar,
                            state_snapshot_capture,
                        ),
                    });
                    next_event_index = next_event_index.saturating_add(1);
                    previous_timestamp_utc_nanos = timestamp_utc_nanos;
                }
                IntervalStochasticUpdate::ExogenousNews {
                    timestamp_utc_nanos,
                    ..
                } => {
                    previous_timestamp_utc_nanos = timestamp_utc_nanos;
                }
            }
        }

        let elapsed_hours_to_interval_end =
            derive_elapsed_hours(previous_timestamp_utc_nanos, interval_end_utc_nanos);

        next_state = evolve_hawkes_state_for_simulation_step(
            next_state,
            run_config.model_inputs.kernel_parameters,
            HawkesTransitionStep::new(elapsed_hours_to_interval_end, None, None),
        )
        .map_err(map_transition_error_to_seeded_simulation_run_error)?;
    }

    let horizon_end_utc_nanos = run_config
        .model_inputs
        .hourly_factor_contract
        .interval_end_utc_nanos[run_config.horizon_interval_count - 1];

    Ok(SeededTickSimulationRunResult {
        schema_version: SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION,
        determinism_policy: SEEDED_TICK_EVENT_DETERMINISM_POLICY,
        event_ordering_policy: SEEDED_TICK_EVENT_ORDERING_POLICY,
        randomness_stream_policy: SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY,
        randomness_substream_derivation_policy:
            SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
        seed: run_config.seed,
        start_time_utc: run_config.start_time_utc,
        end_time_utc: DateTime::<Utc>::from_timestamp_nanos(horizon_end_utc_nanos),
        simulated_interval_count: run_config.horizon_interval_count,
        event_tape_metadata,
        event_path,
        final_excitation_state: next_state,
        debug_trace_enabled: run_config.options.debug_trace_enabled,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum IntervalStochasticUpdate {
    EndogenousEvent {
        timestamp_utc_nanos: i64,
        side: SeededTickEventSide,
        generation_order: u64,
    },
    ExogenousNews {
        timestamp_utc_nanos: i64,
        severity_mark: f64,
        generation_order: u64,
    },
}

impl IntervalStochasticUpdate {
    const fn timestamp_utc_nanos(&self) -> i64 {
        match *self {
            Self::EndogenousEvent {
                timestamp_utc_nanos,
                ..
            } => timestamp_utc_nanos,
            Self::ExogenousNews {
                timestamp_utc_nanos,
                ..
            } => timestamp_utc_nanos,
        }
    }

    const fn ordering_key(&self) -> (i64, u8, u8, u64) {
        match *self {
            Self::EndogenousEvent {
                timestamp_utc_nanos,
                side,
                generation_order,
            } => (
                timestamp_utc_nanos,
                0,
                match side {
                    SeededTickEventSide::Buy => 0,
                    SeededTickEventSide::Sell => 1,
                },
                generation_order,
            ),
            Self::ExogenousNews {
                timestamp_utc_nanos,
                generation_order,
                ..
            } => (timestamp_utc_nanos, 1, 0, generation_order),
        }
    }

    const fn transition_step(
        &self,
        elapsed_hours_since_previous_update: f64,
    ) -> HawkesTransitionStep {
        match *self {
            Self::EndogenousEvent { side, .. } => HawkesTransitionStep::new(
                elapsed_hours_since_previous_update,
                Some(map_seeded_tick_side_to_hawkes_side(side)),
                None,
            ),
            Self::ExogenousNews { severity_mark, .. } => HawkesTransitionStep::new(
                elapsed_hours_since_previous_update,
                None,
                Some(severity_mark),
            ),
        }
    }
}

fn schedule_interval_stochastic_updates<F>(
    seed: u64,
    interval_index: usize,
    interval_start_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_duration_in_hours: f64,
    interval_projection: HawkesIntensityProjection,
    interval_news_intensity: f64,
    randomness_factory: &F,
) -> Vec<IntervalStochasticUpdate>
where
    F: SimulationRandomnessFactory,
{
    let mut buy_event_count_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount,
        interval_index as u64,
    );
    let mut sell_event_count_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousSellEventCount,
        interval_index as u64,
    );
    let mut buy_event_timestamp_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp,
        interval_index as u64,
    );
    let mut sell_event_timestamp_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp,
        interval_index as u64,
    );
    let mut news_count_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsCount,
        interval_index as u64,
    );
    let mut news_timestamp_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp,
        interval_index as u64,
    );
    let mut news_severity_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity,
        interval_index as u64,
    );

    let interval_buy_intensity = interval_projection.seasonality_scaled.lambda_plus.max(0.0);
    let interval_sell_intensity = interval_projection.seasonality_scaled.lambda_minus.max(0.0);

    let interval_buy_event_count = sample_count_from_expectation(
        interval_buy_intensity * interval_duration_in_hours,
        &mut buy_event_count_stream,
    );
    let interval_sell_event_count = sample_count_from_expectation(
        interval_sell_intensity * interval_duration_in_hours,
        &mut sell_event_count_stream,
    );
    let interval_news_count = sample_count_from_expectation(
        interval_news_intensity * interval_duration_in_hours,
        &mut news_count_stream,
    );

    let mut interval_updates: Vec<IntervalStochasticUpdate> = Vec::with_capacity(
        (interval_buy_event_count + interval_sell_event_count + interval_news_count) as usize,
    );
    let mut next_generation_order = 0_u64;

    for _buy_event_position in 0..interval_buy_event_count {
        let timestamp_utc_nanos = sample_interval_timestamp_utc_nanos(
            interval_start_utc_nanos,
            interval_duration_nanos,
            &mut buy_event_timestamp_stream,
        );

        interval_updates.push(IntervalStochasticUpdate::EndogenousEvent {
            timestamp_utc_nanos,
            side: SeededTickEventSide::Buy,
            generation_order: next_generation_order,
        });
        next_generation_order = next_generation_order.saturating_add(1);
    }

    for _sell_event_position in 0..interval_sell_event_count {
        let timestamp_utc_nanos = sample_interval_timestamp_utc_nanos(
            interval_start_utc_nanos,
            interval_duration_nanos,
            &mut sell_event_timestamp_stream,
        );

        interval_updates.push(IntervalStochasticUpdate::EndogenousEvent {
            timestamp_utc_nanos,
            side: SeededTickEventSide::Sell,
            generation_order: next_generation_order,
        });
        next_generation_order = next_generation_order.saturating_add(1);
    }

    for _news_position in 0..interval_news_count {
        let timestamp_utc_nanos = sample_interval_timestamp_utc_nanos(
            interval_start_utc_nanos,
            interval_duration_nanos,
            &mut news_timestamp_stream,
        );
        let severity_mark = sample_exogenous_news_severity_mark(&mut news_severity_stream);

        interval_updates.push(IntervalStochasticUpdate::ExogenousNews {
            timestamp_utc_nanos,
            severity_mark,
            generation_order: next_generation_order,
        });
        next_generation_order = next_generation_order.saturating_add(1);
    }

    interval_updates.sort_by_key(IntervalStochasticUpdate::ordering_key);
    interval_updates
}

fn map_transition_error_to_seeded_simulation_run_error(
    source: HawkesTransitionError,
) -> SeededTickSimulationRunError {
    match source {
        HawkesTransitionError::InvalidNewsSeverityMark { source } => {
            SeededTickSimulationRunError::InvalidGeneratedNewsSeverityMark {
                value: source.value,
            }
        }
        _ => SeededTickSimulationRunError::InvalidTransitionStep { source },
    }
}

const fn build_event_state_snapshot(
    state: HawkesExcitationState,
    lambda_bar: f64,
    snapshot_capture: SeededTickEventStateSnapshotCapture,
) -> Option<SeededTickEventStateSnapshot> {
    match snapshot_capture {
        SeededTickEventStateSnapshotCapture::Disabled => None,
        SeededTickEventStateSnapshotCapture::PostEventTransition => {
            Some(SeededTickEventStateSnapshot::from_state(state, lambda_bar))
        }
    }
}

const fn build_seeded_tick_event_tape_metadata(
    state_snapshot_capture: SeededTickEventStateSnapshotCapture,
) -> SeededTickEventTapeMetadata {
    SeededTickEventTapeMetadata {
        schema_version: SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
        event_record_schema_version: SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION,
        state_snapshot_schema_version: SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION,
        event_ordering_policy: SEEDED_TICK_EVENT_ORDERING_POLICY,
        timestamp_representation_policy: SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY,
        side_encoding_policy: SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY,
        state_snapshot_capture,
        state_snapshot_capture_point_policy: state_snapshot_capture.capture_point_policy(),
    }
}

fn derive_elapsed_hours(previous_timestamp_utc_nanos: i64, timestamp_utc_nanos: i64) -> f64 {
    (timestamp_utc_nanos - previous_timestamp_utc_nanos) as f64 / ONE_HOUR_NANOS as f64
}

fn sample_interval_timestamp_utc_nanos<R>(
    interval_start_utc_nanos: i64,
    interval_duration_nanos: i64,
    randomness_stream: &mut R,
) -> i64
where
    R: SimulationRandomnessStream,
{
    if interval_duration_nanos <= 1 {
        return interval_start_utc_nanos;
    }

    let timestamp_offset_nanos = (randomness_stream.next_unit_interval_f64()
        * interval_duration_nanos as f64)
        .floor() as i64;

    interval_start_utc_nanos + timestamp_offset_nanos.min(interval_duration_nanos - 1)
}

fn sample_count_from_expectation<R>(expected_count: f64, randomness_stream: &mut R) -> u64
where
    R: SimulationRandomnessStream,
{
    if !expected_count.is_finite() || expected_count <= 0.0 {
        return 0;
    }

    if expected_count >= u64::MAX as f64 {
        return MAX_STOCHASTIC_UPDATES_PER_INTERVAL;
    }

    let floored_count = expected_count.floor() as u64;
    let fractional_count = expected_count - floored_count as f64;

    let sampled_count = if randomness_stream.next_unit_interval_f64() < fractional_count {
        floored_count.saturating_add(1)
    } else {
        floored_count
    };

    sampled_count.min(MAX_STOCHASTIC_UPDATES_PER_INTERVAL)
}

fn sample_exogenous_news_severity_mark<R>(randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream,
{
    let unit_draw = randomness_stream.next_unit_interval_f64();
    -(1.0 - unit_draw).ln()
}

const fn map_seeded_tick_side_to_hawkes_side(side: SeededTickEventSide) -> HawkesEventSide {
    match side {
        SeededTickEventSide::Buy => HawkesEventSide::Buy,
        SeededTickEventSide::Sell => HawkesEventSide::Sell,
    }
}

fn validate_hourly_factor_field_length(
    field_name: &'static str,
    expected_interval_count: usize,
    observed_interval_count: usize,
) -> Result<(), SeededTickSimulationRunError> {
    if observed_interval_count != expected_interval_count {
        return Err(
            SeededTickSimulationRunError::HourlyFactorFieldLengthMismatch {
                field_name,
                expected_interval_count,
                observed_interval_count,
            },
        );
    }

    Ok(())
}

fn validate_hourly_factor_interval_coverage(
    interval_start_utc_nanos: &[i64],
    interval_end_utc_nanos: &[i64],
    simulated_interval_count: usize,
) -> Result<(), SeededTickSimulationRunError> {
    for interval_index in 0..simulated_interval_count {
        let current_interval_start_utc_nanos = interval_start_utc_nanos[interval_index];
        let current_interval_end_utc_nanos = interval_end_utc_nanos[interval_index];

        if current_interval_end_utc_nanos <= current_interval_start_utc_nanos {
            return Err(
                SeededTickSimulationRunError::InvalidHourlyFactorIntervalBounds {
                    interval_index,
                    interval_start_utc_nanos: current_interval_start_utc_nanos,
                    interval_end_utc_nanos: current_interval_end_utc_nanos,
                },
            );
        }

        if interval_index == 0 {
            continue;
        }

        let previous_interval_index = interval_index - 1;
        let previous_interval_end_utc_nanos = interval_end_utc_nanos[previous_interval_index];
        if current_interval_start_utc_nanos != previous_interval_end_utc_nanos {
            return Err(
                SeededTickSimulationRunError::NonConsecutiveHourlyFactorCoverage {
                    previous_interval_index,
                    previous_interval_end_utc_nanos,
                    current_interval_index: interval_index,
                    current_interval_start_utc_nanos: current_interval_start_utc_nanos,
                },
            );
        }
    }

    Ok(())
}

/// Resolve interval-aligned hourly factors for the simulation path.
pub fn load_hourly_factor_contract_for_simulation(
    calendar_registry: &ExchangeCalendarRegistry,
    provider_request: HourlyFactorProviderRequest<'_>,
) -> Result<HourlyFactorContractOutput, HourlyFactorProviderError> {
    provide_hourly_factor_contract(calendar_registry, provider_request)
}

/// Project state to interval intensities for the fine-scale simulation path.
pub fn project_hawkes_intensities_for_simulation_interval(
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

/// Apply one simulation-path Hawkes state transition using canonical call order.
///
/// Call order is delegated to `HawkesExcitationState::apply_transition_step`:
/// drift, then optional endogenous jump, then optional exogenous jump.
pub fn evolve_hawkes_state_for_simulation_step(
    state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    transition_step: HawkesTransitionStep,
) -> Result<HawkesExcitationState, HawkesTransitionError> {
    state.apply_transition_step(kernel_parameters, transition_step)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use serde_json::Value;

    use crate::hawkes::{
        HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters, HawkesTransitionStep,
    };
    use crate::hourly_factors::{
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD, HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
        HOURLY_FACTOR_FALL_BACK_POLICY, HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
        HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY, HOURLY_FACTOR_SPRING_FORWARD_POLICY,
        HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY, HourlyFactorContractDiagnostics,
        HourlyFactorContractOutput, HourlyFactorMaterializationMetadata,
    };
    use crate::randomness::{
        SimulationRandomnessFactory, SimulationRandomnessStream,
        SimulationRandomnessSubstreamDomain,
    };
    use crate::simulation::{
        SEEDED_TICK_EVENT_DETERMINISM_POLICY, SEEDED_TICK_EVENT_ORDERING_POLICY,
        SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION, SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY,
        SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
        SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION, SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY,
        SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY,
        SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION,
        SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
        SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY, SeededTickEventSide,
        SeededTickEventStateSnapshotCapture, SeededTickSimulationModelInputs,
        SeededTickSimulationRunConfig, SeededTickSimulationRunError,
        SeededTickSimulationRunOptions, run_seeded_tick_event_simulation,
        run_seeded_tick_event_simulation_with_randomness_factory,
    };

    const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;

    #[test]
    fn seeded_simulation_run_executes_with_valid_config() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            9_101,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: true,
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect("valid run config should initialize");

        let first_result = run_seeded_tick_event_simulation(run_config.clone())
            .expect("valid run should produce deterministic output");
        let second_result = run_seeded_tick_event_simulation(run_config)
            .expect("same run config should produce deterministic output");

        assert_eq!(first_result, second_result);
        assert_eq!(
            first_result.schema_version,
            SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION
        );
        assert_eq!(
            first_result.determinism_policy,
            SEEDED_TICK_EVENT_DETERMINISM_POLICY
        );
        assert_eq!(
            first_result.event_ordering_policy,
            SEEDED_TICK_EVENT_ORDERING_POLICY
        );
        assert_eq!(
            first_result.randomness_stream_policy,
            SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY
        );
        assert_eq!(
            first_result.randomness_substream_derivation_policy,
            SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY
        );
        assert_eq!(first_result.seed, 9_101);
        assert_eq!(first_result.simulated_interval_count, 2);
        assert!(first_result.debug_trace_enabled);
        assert!(!first_result.event_path.is_empty());
        assert!(is_non_decreasing_timestamp_path(&first_result));
    }

    #[test]
    fn seeded_simulation_run_diverges_with_different_seeds() {
        let first_run_config = SeededTickSimulationRunConfig::try_new(
            9_101,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            None,
        )
        .expect("first run config should initialize");
        let second_run_config = SeededTickSimulationRunConfig::try_new(
            9_102,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            None,
        )
        .expect("second run config should initialize");

        let first_result = run_seeded_tick_event_simulation(first_run_config)
            .expect("first deterministic run should complete");
        let second_result = run_seeded_tick_event_simulation(second_run_config)
            .expect("second deterministic run should complete");

        assert_ne!(first_result.event_path, second_result.event_path);
    }

    #[test]
    fn seeded_simulation_run_is_independent_from_debug_trace_flag() {
        let run_config_without_debug = SeededTickSimulationRunConfig::try_new(
            8_001,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: false,
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect("run config without debug trace should initialize");
        let run_config_with_debug = SeededTickSimulationRunConfig::try_new(
            8_001,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: true,
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect("run config with debug trace should initialize");

        let result_without_debug = run_seeded_tick_event_simulation(run_config_without_debug)
            .expect("run without debug trace should complete");
        let result_with_debug = run_seeded_tick_event_simulation(run_config_with_debug)
            .expect("run with debug trace should complete");

        assert_eq!(
            result_without_debug.event_path,
            result_with_debug.event_path
        );
        assert_eq!(
            result_without_debug.final_excitation_state,
            result_with_debug.final_excitation_state
        );
    }

    #[test]
    fn seeded_simulation_run_accepts_injected_randomness_factory() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            5_003,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            None,
        )
        .expect("run config should initialize");
        let fixture_factory = FixtureRandomnessFactory;

        let first_result = run_seeded_tick_event_simulation_with_randomness_factory(
            run_config.clone(),
            &fixture_factory,
        )
        .expect("injected fixture randomness run should complete");
        let second_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &fixture_factory)
                .expect("second injected fixture randomness run should complete");

        assert_eq!(first_result.event_path, second_result.event_path);
        assert!(!first_result.event_path.is_empty());
        assert!(is_non_decreasing_timestamp_path(&first_result));
    }

    #[test]
    fn seeded_simulation_run_rejects_zero_horizon() {
        let error = SeededTickSimulationRunConfig::try_new(
            42,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            0,
            valid_model_inputs(),
            None,
        )
        .expect_err("zero horizon must fail");

        assert!(matches!(
            error,
            SeededTickSimulationRunError::InvalidHorizonIntervalCount { value: 0 }
        ));
    }

    #[test]
    fn seeded_simulation_run_rejects_incomplete_hourly_factor_payload() {
        let mut model_inputs = valid_model_inputs();
        model_inputs.hourly_factor_contract.s_h.pop();

        let error = SeededTickSimulationRunConfig::try_new(
            77,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            model_inputs,
            None,
        )
        .expect_err("incomplete hourly factor vectors must fail");

        assert!(matches!(
            error,
            SeededTickSimulationRunError::HourlyFactorFieldLengthMismatch {
                field_name: "s_h",
                expected_interval_count: 2,
                observed_interval_count: 1,
            }
        ));
    }

    #[test]
    fn seeded_simulation_run_rejects_non_consecutive_hourly_factor_coverage() {
        let mut model_inputs = valid_model_inputs();
        let start_utc_nanos = 1_704_067_200_000_000_000;

        model_inputs.hourly_factor_contract.interval_start_utc_nanos[1] += ONE_HOUR_NANOS;
        model_inputs.hourly_factor_contract.interval_end_utc_nanos[1] += ONE_HOUR_NANOS;
        model_inputs.hourly_factor_contract.request_end_utc_nanos += ONE_HOUR_NANOS;
        model_inputs
            .hourly_factor_contract
            .diagnostics
            .last_interval_end_utc_nanos += ONE_HOUR_NANOS;

        let error = SeededTickSimulationRunConfig::try_new(
            88,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            2,
            model_inputs,
            None,
        )
        .expect_err("non-consecutive interval coverage must fail");

        assert!(matches!(
            error,
            SeededTickSimulationRunError::NonConsecutiveHourlyFactorCoverage {
                previous_interval_index: 0,
                previous_interval_end_utc_nanos,
                current_interval_index: 1,
                current_interval_start_utc_nanos,
            } if previous_interval_end_utc_nanos == start_utc_nanos + ONE_HOUR_NANOS
                && current_interval_start_utc_nanos == start_utc_nanos + 2 * ONE_HOUR_NANOS
        ));
    }

    #[test]
    fn seeded_simulation_preserves_full_state_carryover_across_hour_boundaries() {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let model_inputs = two_interval_zero_event_carryover_model_inputs();
        let kernel_parameters = model_inputs.kernel_parameters;

        let one_hour_config = SeededTickSimulationRunConfig::try_new(
            10_052,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            1,
            model_inputs.clone(),
            None,
        )
        .expect("one-hour carryover run config should initialize");

        let two_hour_config = SeededTickSimulationRunConfig::try_new(
            10_052,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            2,
            model_inputs,
            None,
        )
        .expect("two-hour carryover run config should initialize");

        let one_hour_result = run_seeded_tick_event_simulation(one_hour_config)
            .expect("one-hour carryover run should complete");
        let two_hour_result = run_seeded_tick_event_simulation(two_hour_config)
            .expect("two-hour carryover run should complete");

        let expected_two_hour_final_state = super::evolve_hawkes_state_for_simulation_step(
            one_hour_result.final_excitation_state,
            kernel_parameters,
            HawkesTransitionStep::new(1.0, None, None),
        )
        .expect("deterministic carryover drift step should succeed");

        assert!(one_hour_result.event_path.is_empty());
        assert!(two_hour_result.event_path.is_empty());
        assert_float_close(
            two_hour_result.final_excitation_state.y_plus_plus,
            expected_two_hour_final_state.y_plus_plus,
        );
        assert_float_close(
            two_hour_result.final_excitation_state.y_plus_minus,
            expected_two_hour_final_state.y_plus_minus,
        );
        assert_float_close(
            two_hour_result.final_excitation_state.y_minus_plus,
            expected_two_hour_final_state.y_minus_plus,
        );
        assert_float_close(
            two_hour_result.final_excitation_state.y_minus_minus,
            expected_two_hour_final_state.y_minus_minus,
        );
    }

    #[test]
    fn seeded_simulation_factor_changes_apply_on_hour_boundaries_only() {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let boundary_utc_nanos = start_utc_nanos + ONE_HOUR_NANOS;

        let baseline_run_config = SeededTickSimulationRunConfig::try_new(
            10_053,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            2,
            two_interval_constant_intensity_model_inputs(1.0),
            None,
        )
        .expect("baseline boundary-factor run config should initialize");

        let muted_second_hour_run_config = SeededTickSimulationRunConfig::try_new(
            10_053,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            2,
            two_interval_constant_intensity_model_inputs(0.0),
            None,
        )
        .expect("muted boundary-factor run config should initialize");

        let baseline_result = run_seeded_tick_event_simulation(baseline_run_config)
            .expect("baseline boundary-factor run should complete");
        let muted_second_hour_result =
            run_seeded_tick_event_simulation(muted_second_hour_run_config)
                .expect("muted boundary-factor run should complete");

        assert_eq!(baseline_result.event_path.len(), 8);
        assert_eq!(muted_second_hour_result.event_path.len(), 4);
        assert!(
            muted_second_hour_result
                .event_path
                .iter()
                .all(|event_record| event_record.timestamp_utc_nanos < boundary_utc_nanos)
        );
        assert_eq!(
            &baseline_result.event_path[..muted_second_hour_result.event_path.len()],
            muted_second_hour_result.event_path.as_slice()
        );
    }

    #[test]
    fn seeded_simulation_scheduler_interleaves_buy_and_sell_arrivals() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            1_007,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(2.0, 2.0, 0.0),
            None,
        )
        .expect("interleaved schedule run config should initialize");

        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![0.0],
            vec![0.75, 0.25],
            vec![0.5, 0.1],
            vec![0.0],
            vec![],
            vec![],
        );
        let run_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
                .expect("interleaved schedule run should complete");

        let observed_sequence: Vec<(i64, SeededTickEventSide)> = run_result
            .event_path
            .iter()
            .map(|event_record| (event_record.timestamp_utc_nanos, event_record.side))
            .collect();

        let interval_start_utc_nanos = 1_704_067_200_000_000_000;
        let expected_sequence = vec![
            (
                interval_start_utc_nanos + ONE_HOUR_NANOS / 10,
                SeededTickEventSide::Sell,
            ),
            (
                interval_start_utc_nanos + ONE_HOUR_NANOS / 4,
                SeededTickEventSide::Buy,
            ),
            (
                interval_start_utc_nanos + ONE_HOUR_NANOS / 2,
                SeededTickEventSide::Sell,
            ),
            (
                interval_start_utc_nanos + 3 * ONE_HOUR_NANOS / 4,
                SeededTickEventSide::Buy,
            ),
        ];

        assert_eq!(observed_sequence, expected_sequence);
        assert!(is_non_decreasing_timestamp_path(&run_result));
    }

    #[test]
    fn seeded_simulation_scheduler_applies_buy_before_sell_tie_ordering_policy() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            2_008,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(1.0, 1.0, 0.0),
            None,
        )
        .expect("tie-ordering run config should initialize");

        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![0.0],
            vec![0.3],
            vec![0.3],
            vec![0.0],
            vec![],
            vec![],
        );
        let run_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
                .expect("tie-ordering run should complete");

        assert_eq!(run_result.event_path.len(), 2);
        assert_eq!(
            run_result.event_path[0].timestamp_utc_nanos,
            run_result.event_path[1].timestamp_utc_nanos
        );
        assert_eq!(run_result.event_path[0].side, SeededTickEventSide::Buy);
        assert_eq!(run_result.event_path[1].side, SeededTickEventSide::Sell);
        assert_eq!(
            run_result.event_ordering_policy,
            SEEDED_TICK_EVENT_ORDERING_POLICY
        );
    }

    #[test]
    fn seeded_simulation_boundary_adjacent_event_times_are_not_snapped_to_hour_edges() {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let boundary_utc_nanos = start_utc_nanos + ONE_HOUR_NANOS;

        let run_config = SeededTickSimulationRunConfig::try_new(
            10_054,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            2,
            two_interval_boundary_adjacent_model_inputs(),
            None,
        )
        .expect("boundary-adjacent run config should initialize");

        let run_result = run_seeded_tick_event_simulation_with_randomness_factory(
            run_config,
            &BoundaryAdjacentRandomnessFactory,
        )
        .expect("boundary-adjacent run should complete");

        assert_eq!(run_result.event_path.len(), 2);

        let first_timestamp_utc_nanos = run_result.event_path[0].timestamp_utc_nanos;
        let second_timestamp_utc_nanos = run_result.event_path[1].timestamp_utc_nanos;

        assert!(first_timestamp_utc_nanos < boundary_utc_nanos);
        assert!(boundary_utc_nanos - first_timestamp_utc_nanos <= 4);
        assert!(second_timestamp_utc_nanos > boundary_utc_nanos);
        assert!(second_timestamp_utc_nanos - boundary_utc_nanos <= 4);
        assert_ne!(first_timestamp_utc_nanos, boundary_utc_nanos);
        assert_ne!(second_timestamp_utc_nanos, boundary_utc_nanos);
    }

    #[test]
    fn seeded_simulation_state_snapshot_captures_post_event_transition_boundary() {
        let mut model_inputs = one_interval_model_inputs(0.5, 0.5, 0.0);
        model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
            0.5,
            1.5,
            HawkesKernelLoadings {
                plus_plus: 0.4,
                plus_minus: 0.0,
                minus_plus: 0.2,
                minus_minus: 0.0,
            },
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
        )
        .expect("snapshot boundary kernel fixture should be valid");
        model_inputs.initial_excitation_state = HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        };

        let run_config = SeededTickSimulationRunConfig::try_new(
            7_047,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            model_inputs,
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: false,
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::PostEventTransition,
            }),
        )
        .expect("snapshot boundary run config should initialize");
        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![1.0],
            vec![0.0],
            vec![],
            vec![0.0],
            vec![],
            vec![],
        );
        let run_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
                .expect("snapshot boundary run should complete");

        assert_eq!(run_result.event_path.len(), 1);
        assert_eq!(
            run_result.event_tape_metadata.state_snapshot_capture,
            SeededTickEventStateSnapshotCapture::PostEventTransition
        );
        assert_eq!(
            run_result
                .event_tape_metadata
                .state_snapshot_capture_point_policy,
            Some(SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY)
        );

        let snapshot = run_result.event_path[0]
            .state_snapshot
            .expect("snapshot capture must populate state_snapshot");

        assert_float_close(snapshot.y_plus_plus, 0.4);
        assert_float_close(snapshot.y_plus_minus, 0.0);
        assert_float_close(snapshot.y_minus_plus, 0.2);
        assert_float_close(snapshot.y_minus_minus, 0.0);
        assert_float_close(snapshot.deseasonalized_lambda_plus, 0.9);
        assert_float_close(snapshot.deseasonalized_lambda_minus, 0.7);
    }

    #[test]
    fn seeded_simulation_event_tape_schema_exposes_required_and_optional_fields() {
        let run_config_without_snapshot = SeededTickSimulationRunConfig::try_new(
            8_048,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(1.0, 0.5, 0.0),
            None,
        )
        .expect("schema run config without snapshot should initialize");

        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![1.0],
            vec![0.25],
            vec![],
            vec![0.0],
            vec![],
            vec![],
        );

        let run_result_without_snapshot = run_seeded_tick_event_simulation_with_randomness_factory(
            run_config_without_snapshot,
            &scripted_factory,
        )
        .expect("schema run without snapshot should complete");

        let serialized_without_snapshot = run_result_without_snapshot
            .serialize_event_tape_json()
            .expect("event tape serialization without snapshot should succeed");
        let payload_without_snapshot: Value = serde_json::from_str(&serialized_without_snapshot)
            .expect("serialized event tape payload should parse");

        assert_eq!(
            payload_without_snapshot["schema_version"],
            SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION
        );
        assert_eq!(
            payload_without_snapshot["metadata"]["schema_version"],
            SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION
        );
        assert_eq!(
            payload_without_snapshot["metadata"]["event_record_schema_version"],
            SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION
        );
        assert_eq!(
            payload_without_snapshot["metadata"]["state_snapshot_schema_version"],
            SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION
        );
        assert_eq!(
            payload_without_snapshot["metadata"]["timestamp_representation_policy"],
            SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY
        );
        assert_eq!(
            payload_without_snapshot["metadata"]["side_encoding_policy"],
            SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY
        );
        assert_eq!(
            payload_without_snapshot["metadata"]["state_snapshot_capture"],
            "disabled"
        );
        assert!(
            payload_without_snapshot["metadata"]
                .get("state_snapshot_capture_point_policy")
                .is_none()
        );

        let serialized_events_without_snapshot = payload_without_snapshot["event_tape"]
            .as_array()
            .expect("event_tape must serialize as an array");
        assert_eq!(serialized_events_without_snapshot.len(), 1);

        let first_event_without_snapshot = serialized_events_without_snapshot[0]
            .as_object()
            .expect("event record must serialize as an object");

        for required_field in ["event_index", "timestamp_utc_nanos", "side"] {
            assert!(first_event_without_snapshot.contains_key(required_field));
        }

        assert!(!first_event_without_snapshot.contains_key("state_snapshot"));
        assert!(!first_event_without_snapshot.contains_key("interval_index"));

        let run_config_with_snapshot = SeededTickSimulationRunConfig::try_new(
            8_048,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(1.0, 0.5, 0.0),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: false,
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::PostEventTransition,
            }),
        )
        .expect("schema run config with snapshot should initialize");

        let run_result_with_snapshot = run_seeded_tick_event_simulation_with_randomness_factory(
            run_config_with_snapshot,
            &scripted_factory,
        )
        .expect("schema run with snapshot should complete");

        let serialized_with_snapshot = run_result_with_snapshot
            .serialize_event_tape_json()
            .expect("event tape serialization with snapshot should succeed");
        let payload_with_snapshot: Value = serde_json::from_str(&serialized_with_snapshot)
            .expect("serialized event tape payload should parse");

        assert_eq!(
            payload_with_snapshot["metadata"]["state_snapshot_capture"],
            "post_event_transition"
        );
        assert_eq!(
            payload_with_snapshot["metadata"]["state_snapshot_capture_point_policy"],
            SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY
        );

        let first_event_with_snapshot = payload_with_snapshot["event_tape"][0]
            .as_object()
            .expect("event record with snapshot must serialize as an object");
        let state_snapshot = first_event_with_snapshot
            .get("state_snapshot")
            .expect("event record must include state_snapshot when capture is enabled")
            .as_object()
            .expect("state_snapshot must serialize as an object");

        for required_snapshot_field in [
            "y_plus_plus",
            "y_plus_minus",
            "y_minus_plus",
            "y_minus_minus",
            "deseasonalized_lambda_plus",
            "deseasonalized_lambda_minus",
        ] {
            assert!(state_snapshot.contains_key(required_snapshot_field));
        }
    }

    #[test]
    fn seeded_simulation_event_tape_serialization_is_deterministic() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            9_049,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: false,
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::PostEventTransition,
            }),
        )
        .expect("serialization determinism run config should initialize");

        let first_result = run_seeded_tick_event_simulation(run_config.clone())
            .expect("first serialization determinism run should complete");
        let second_result = run_seeded_tick_event_simulation(run_config)
            .expect("second serialization determinism run should complete");

        let first_json = first_result
            .serialize_event_tape_json()
            .expect("first event tape serialization should succeed");
        let second_json = second_result
            .serialize_event_tape_json()
            .expect("second event tape serialization should succeed");

        assert_eq!(first_json, second_json);
    }

    #[test]
    fn seeded_simulation_tied_and_near_tied_timestamps_serialize_deterministically() {
        let tie_run_config = SeededTickSimulationRunConfig::try_new(
            10_050,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(1.0, 1.0, 0.0),
            None,
        )
        .expect("tie serialization run config should initialize");

        let tie_scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![0.0],
            vec![0.3],
            vec![0.3],
            vec![0.0],
            vec![],
            vec![],
        );

        let tie_result = run_seeded_tick_event_simulation_with_randomness_factory(
            tie_run_config,
            &tie_scripted_factory,
        )
        .expect("tie serialization run should complete");

        let tie_payload: Value = serde_json::from_str(
            &tie_result
                .serialize_event_tape_json()
                .expect("tie event tape serialization should succeed"),
        )
        .expect("tie serialized payload should parse");

        let tie_events = tie_payload["event_tape"]
            .as_array()
            .expect("tie event_tape must serialize as an array");

        assert_eq!(tie_events.len(), 2);
        assert_eq!(
            tie_events[0]["timestamp_utc_nanos"],
            tie_events[1]["timestamp_utc_nanos"]
        );
        assert_eq!(tie_events[0]["side"], "buy");
        assert_eq!(tie_events[1]["side"], "sell");
        assert_eq!(
            tie_payload["metadata"]["timestamp_representation_policy"],
            SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY
        );
        assert!(tie_events[0].get("interval_index").is_none());
        assert!(tie_events[1].get("interval_index").is_none());

        let near_tie_run_config = SeededTickSimulationRunConfig::try_new(
            10_051,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(1.0, 1.0, 0.0),
            None,
        )
        .expect("near-tie serialization run config should initialize");

        let near_tie_scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![0.0],
            vec![0.300000001],
            vec![0.3],
            vec![0.0],
            vec![],
            vec![],
        );

        let near_tie_result = run_seeded_tick_event_simulation_with_randomness_factory(
            near_tie_run_config,
            &near_tie_scripted_factory,
        )
        .expect("near-tie serialization run should complete");

        let near_tie_payload: Value = serde_json::from_str(
            &near_tie_result
                .serialize_event_tape_json()
                .expect("near-tie event tape serialization should succeed"),
        )
        .expect("near-tie serialized payload should parse");

        let near_tie_events = near_tie_payload["event_tape"]
            .as_array()
            .expect("near-tie event_tape must serialize as an array");

        assert_eq!(near_tie_events.len(), 2);

        let first_timestamp = near_tie_events[0]["timestamp_utc_nanos"]
            .as_i64()
            .expect("first near-tie timestamp should parse");
        let second_timestamp = near_tie_events[1]["timestamp_utc_nanos"]
            .as_i64()
            .expect("second near-tie timestamp should parse");

        assert!(first_timestamp < second_timestamp);
        assert_eq!(near_tie_events[0]["side"], "sell");
        assert_eq!(near_tie_events[1]["side"], "buy");
        assert!(
            !near_tie_events[0]
                .as_object()
                .is_some_and(|entry| entry.contains_key("bucket"))
        );
        assert!(
            !near_tie_events[1]
                .as_object()
                .is_some_and(|entry| entry.contains_key("bucket"))
        );
    }

    fn is_non_decreasing_timestamp_path(run_result: &super::SeededTickSimulationRunResult) -> bool {
        run_result.event_path.windows(2).all(|event_window| {
            event_window[0].timestamp_utc_nanos <= event_window[1].timestamp_utc_nanos
        })
    }

    fn assert_float_close(observed: f64, expected: f64) {
        assert!(
            (observed - expected).abs() <= 1e-12,
            "float mismatch, expected {expected}, observed {observed}"
        );
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FixtureRandomnessFactory;

    impl SimulationRandomnessFactory for FixtureRandomnessFactory {
        type Stream = FixtureRandomnessStream;

        fn create_substream(
            &self,
            _root_seed: u64,
            substream_domain: SimulationRandomnessSubstreamDomain,
            substream_index: u64,
        ) -> Self::Stream {
            FixtureRandomnessStream {
                substream_domain,
                substream_index,
                draw_count: 0,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FixtureRandomnessStream {
        substream_domain: SimulationRandomnessSubstreamDomain,
        substream_index: u64,
        draw_count: u64,
    }

    impl SimulationRandomnessStream for FixtureRandomnessStream {
        fn next_u64(&mut self) -> u64 {
            self.draw_count = self.draw_count.saturating_add(1);

            match self.substream_domain {
                SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount => 0,
                SimulationRandomnessSubstreamDomain::EndogenousSellEventCount => {
                    u64::MAX.saturating_sub(1)
                }
                SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp => {
                    self.substream_index.wrapping_add(self.draw_count)
                }
                SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp => {
                    self.substream_index.wrapping_add(self.draw_count)
                }
                SimulationRandomnessSubstreamDomain::ExogenousNewsCount => 0,
                SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp => 0,
                SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity => 0,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct ScriptedRandomnessFactory {
        buy_count_draws: Vec<f64>,
        sell_count_draws: Vec<f64>,
        buy_timestamp_draws: Vec<f64>,
        sell_timestamp_draws: Vec<f64>,
        news_count_draws: Vec<f64>,
        news_timestamp_draws: Vec<f64>,
        news_severity_draws: Vec<f64>,
    }

    impl ScriptedRandomnessFactory {
        fn new(
            buy_count_draws: Vec<f64>,
            sell_count_draws: Vec<f64>,
            buy_timestamp_draws: Vec<f64>,
            sell_timestamp_draws: Vec<f64>,
            news_count_draws: Vec<f64>,
            news_timestamp_draws: Vec<f64>,
            news_severity_draws: Vec<f64>,
        ) -> Self {
            Self {
                buy_count_draws,
                sell_count_draws,
                buy_timestamp_draws,
                sell_timestamp_draws,
                news_count_draws,
                news_timestamp_draws,
                news_severity_draws,
            }
        }
    }

    impl SimulationRandomnessFactory for ScriptedRandomnessFactory {
        type Stream = ScriptedRandomnessStream;

        fn create_substream(
            &self,
            _root_seed: u64,
            substream_domain: SimulationRandomnessSubstreamDomain,
            _substream_index: u64,
        ) -> Self::Stream {
            let draws = match substream_domain {
                SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount => {
                    self.buy_count_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::EndogenousSellEventCount => {
                    self.sell_count_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp => {
                    self.buy_timestamp_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp => {
                    self.sell_timestamp_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::ExogenousNewsCount => {
                    self.news_count_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp => {
                    self.news_timestamp_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity => {
                    self.news_severity_draws.clone()
                }
            };

            ScriptedRandomnessStream::new(draws)
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct ScriptedRandomnessStream {
        draws: Vec<f64>,
        position: usize,
    }

    impl ScriptedRandomnessStream {
        fn new(draws: Vec<f64>) -> Self {
            Self { draws, position: 0 }
        }
    }

    impl SimulationRandomnessStream for ScriptedRandomnessStream {
        fn next_u64(&mut self) -> u64 {
            let unit_draw = self.next_unit_interval_f64();
            (unit_draw.clamp(0.0, 1.0) * u64::MAX as f64).floor() as u64
        }

        fn next_unit_interval_f64(&mut self) -> f64 {
            let next_draw = self.draws.get(self.position).copied().unwrap_or(0.0);
            self.position = self.position.saturating_add(1);
            next_draw.clamp(0.0, 1.0)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct BoundaryAdjacentRandomnessFactory;

    impl SimulationRandomnessFactory for BoundaryAdjacentRandomnessFactory {
        type Stream = ScriptedRandomnessStream;

        fn create_substream(
            &self,
            _root_seed: u64,
            substream_domain: SimulationRandomnessSubstreamDomain,
            substream_index: u64,
        ) -> Self::Stream {
            let draws = match substream_domain {
                SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount => vec![0.0],
                SimulationRandomnessSubstreamDomain::EndogenousSellEventCount => vec![0.0],
                SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp => {
                    if substream_index == 0 {
                        vec![0.999_999_999_999]
                    } else {
                        vec![0.000_000_000_001]
                    }
                }
                SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp => vec![],
                SimulationRandomnessSubstreamDomain::ExogenousNewsCount => vec![0.0],
                SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp => vec![],
                SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity => vec![],
            };

            ScriptedRandomnessStream::new(draws)
        }
    }

    fn one_interval_model_inputs(
        lambda_plus: f64,
        lambda_minus: f64,
        news_intensity: f64,
    ) -> SeededTickSimulationModelInputs {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let interval_start_utc_nanos = vec![start_utc_nanos];
        let interval_end_utc_nanos = vec![start_utc_nanos + ONE_HOUR_NANOS];

        SeededTickSimulationModelInputs {
            kernel_parameters: HawkesKernelParameters::try_new(
                0.5,
                1.5,
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
            .expect("one-interval kernel parameter fixture should be valid"),
            initial_excitation_state: crate::hawkes::HawkesExcitationState {
                y_plus_plus: lambda_plus - 0.5,
                y_plus_minus: 0.0,
                y_minus_plus: lambda_minus - 0.5,
                y_minus_minus: 0.0,
            },
            hourly_factor_contract: HourlyFactorContractOutput {
                schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
                exchange_code: "XNYS".to_string(),
                exchange_timezone_name: "America/New_York".to_string(),
                request_start_utc_nanos: start_utc_nanos,
                request_end_utc_nanos: start_utc_nanos + ONE_HOUR_NANOS,
                interval_indices: vec![0],
                interval_start_utc_nanos: interval_start_utc_nanos.clone(),
                interval_end_utc_nanos: interval_end_utc_nanos.clone(),
                interval_start_exchange_local: vec!["2024-01-22T09:00:00-05:00".to_string()],
                interval_end_exchange_local: vec!["2024-01-22T10:00:00-05:00".to_string()],
                s_h: vec![1.0],
                nu_h: vec![news_intensity],
                metadata: HourlyFactorMaterializationMetadata {
                    seasonality_materialization_method:
                        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD.to_string(),
                    news_intensity_materialization_method:
                        DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD.to_string(),
                    interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
                    interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
                    timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
                    spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
                    fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
                },
                diagnostics: HourlyFactorContractDiagnostics {
                    interval_count: 1,
                    first_interval_start_utc_nanos: interval_start_utc_nanos[0],
                    last_interval_end_utc_nanos: interval_end_utc_nanos[0],
                    skipped_local_hour_interval_count: 0,
                    repeated_local_hour_interval_count: 0,
                },
            },
        }
    }

    fn two_interval_zero_event_carryover_model_inputs() -> SeededTickSimulationModelInputs {
        let mut model_inputs = valid_model_inputs();

        model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
            0.5,
            1.25,
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
        .expect("zero-event carryover kernel parameter fixture should be valid");

        model_inputs.initial_excitation_state = HawkesExcitationState {
            y_plus_plus: 1.6,
            y_plus_minus: 0.8,
            y_minus_plus: 1.2,
            y_minus_minus: 0.4,
        };

        model_inputs.hourly_factor_contract.s_h = vec![0.0, 0.0];
        model_inputs.hourly_factor_contract.nu_h = vec![0.0, 0.0];

        model_inputs
    }

    fn two_interval_constant_intensity_model_inputs(
        second_interval_seasonality_factor: f64,
    ) -> SeededTickSimulationModelInputs {
        let mut model_inputs = valid_model_inputs();

        model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
            2.0,
            1.0,
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
        .expect("constant-intensity kernel parameter fixture should be valid");

        model_inputs.initial_excitation_state = HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        };

        model_inputs.hourly_factor_contract.s_h = vec![1.0, second_interval_seasonality_factor];
        model_inputs.hourly_factor_contract.nu_h = vec![0.0, 0.0];

        model_inputs
    }

    fn two_interval_boundary_adjacent_model_inputs() -> SeededTickSimulationModelInputs {
        let mut model_inputs = valid_model_inputs();

        model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
            1.0,
            1.0,
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
        .expect("boundary-adjacent kernel parameter fixture should be valid");

        model_inputs.initial_excitation_state = HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: -10.0,
            y_minus_minus: 0.0,
        };

        model_inputs.hourly_factor_contract.s_h = vec![1.0, 1.0];
        model_inputs.hourly_factor_contract.nu_h = vec![0.0, 0.0];

        model_inputs
    }

    fn valid_model_inputs() -> SeededTickSimulationModelInputs {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let interval_start_utc_nanos = vec![start_utc_nanos, start_utc_nanos + ONE_HOUR_NANOS];
        let interval_end_utc_nanos = vec![
            start_utc_nanos + ONE_HOUR_NANOS,
            start_utc_nanos + 2 * ONE_HOUR_NANOS,
        ];

        SeededTickSimulationModelInputs {
            kernel_parameters: HawkesKernelParameters::try_new(
                0.8,
                1.5,
                HawkesKernelLoadings {
                    plus_plus: 0.3,
                    plus_minus: 0.15,
                    minus_plus: 0.1,
                    minus_minus: 0.25,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.05,
                    plus_minus: 0.02,
                    minus_plus: 0.03,
                    minus_minus: 0.06,
                },
            )
            .expect("kernel parameter fixture should be valid"),
            initial_excitation_state: crate::hawkes::HawkesExcitationState {
                y_plus_plus: 0.2,
                y_plus_minus: 0.1,
                y_minus_plus: 0.15,
                y_minus_minus: 0.3,
            },
            hourly_factor_contract: HourlyFactorContractOutput {
                schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
                exchange_code: "XNYS".to_string(),
                exchange_timezone_name: "America/New_York".to_string(),
                request_start_utc_nanos: start_utc_nanos,
                request_end_utc_nanos: start_utc_nanos + 2 * ONE_HOUR_NANOS,
                interval_indices: vec![0, 1],
                interval_start_utc_nanos: interval_start_utc_nanos.clone(),
                interval_end_utc_nanos: interval_end_utc_nanos.clone(),
                interval_start_exchange_local: vec![
                    "2024-01-22T09:00:00-05:00".to_string(),
                    "2024-01-22T10:00:00-05:00".to_string(),
                ],
                interval_end_exchange_local: vec![
                    "2024-01-22T10:00:00-05:00".to_string(),
                    "2024-01-22T11:00:00-05:00".to_string(),
                ],
                s_h: vec![0.95, 1.05],
                nu_h: vec![0.02, 0.03],
                metadata: HourlyFactorMaterializationMetadata {
                    seasonality_materialization_method:
                        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD.to_string(),
                    news_intensity_materialization_method:
                        DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD.to_string(),
                    interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
                    interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
                    timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
                    spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
                    fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
                },
                diagnostics: HourlyFactorContractDiagnostics {
                    interval_count: 2,
                    first_interval_start_utc_nanos: interval_start_utc_nanos[0],
                    last_interval_end_utc_nanos: interval_end_utc_nanos[1],
                    skipped_local_hour_interval_count: 0,
                    repeated_local_hour_interval_count: 0,
                },
            },
        }
    }
}
