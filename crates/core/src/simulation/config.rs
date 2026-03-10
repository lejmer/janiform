//! Seeded simulation run configuration contracts and validation.

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::calendar::ExchangeCalendarRegistry;
use crate::hawkes::{HawkesExcitationState, HawkesKernelParameters};
use crate::hourly_factors::HourlyFactorContractOutput;
use crate::news::NewsProcessConfiguration;
use crate::nig::NigSharedPlusShiftConfiguration;

use super::constants::{
    DEFAULT_DEBUG_TRACE_MAX_RECORDS_PER_CHUNK, DEFAULT_DEBUG_TRACE_SAMPLE_EVERY_NTH_TRANSITION,
    SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY,
};
use super::error::SeededTickSimulationRunError;
use super::runtime::{
    validate_hourly_factor_field_length, validate_hourly_factor_interval_coverage,
    validate_known_scheduled_news_timestamps,
};

/// State snapshot capture mode for per-event diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeededTickEventStateSnapshotCapture {
    Disabled,
    PostEventTransition,
}

impl SeededTickEventStateSnapshotCapture {
    pub(crate) const fn capture_point_policy(self) -> Option<&'static str> {
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

/// Optional debug trace controls for seeded simulation diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeededTickSimulationDebugTraceOptions {
    /// Capture every Nth transition record in deterministic transition order.
    ///
    /// Interval-boundary drift transitions are always recorded.
    pub sample_every_nth_transition: u64,

    /// Maximum number of records in each trace chunk.
    pub max_records_per_chunk: usize,
}

impl SeededTickSimulationDebugTraceOptions {
    fn validate(self) -> Result<(), SeededTickSimulationRunError> {
        if self.sample_every_nth_transition == 0 {
            return Err(
                SeededTickSimulationRunError::InvalidDebugTraceSamplingStride {
                    value: self.sample_every_nth_transition,
                },
            );
        }

        if self.max_records_per_chunk == 0 {
            return Err(SeededTickSimulationRunError::InvalidDebugTraceChunkSize {
                value: self.max_records_per_chunk,
            });
        }

        Ok(())
    }
}

impl Default for SeededTickSimulationDebugTraceOptions {
    fn default() -> Self {
        Self {
            sample_every_nth_transition: DEFAULT_DEBUG_TRACE_SAMPLE_EVERY_NTH_TRANSITION,
            max_records_per_chunk: DEFAULT_DEBUG_TRACE_MAX_RECORDS_PER_CHUNK,
        }
    }
}

/// Optional run settings for seeded tick-event simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeededTickSimulationRunOptions {
    /// Enable optional debug trace artifact emission.
    pub debug_trace_enabled: bool,

    /// Optional debug trace sampling and chunking controls.
    pub debug_trace_options: SeededTickSimulationDebugTraceOptions,

    /// Control optional Hawkes state snapshot capture on each event tape record.
    pub state_snapshot_capture: SeededTickEventStateSnapshotCapture,
}

impl Default for SeededTickSimulationRunOptions {
    fn default() -> Self {
        Self {
            debug_trace_enabled: false,
            debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }
    }
}

/// Required model inputs for seeded fine-path simulation runs.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededTickSimulationModelInputs {
    /// Canonical Hawkes kernel parameter payload.
    pub kernel_parameters: HawkesKernelParameters,

    /// Canonical NIG return decomposition payload shared with the coarse engine.
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,

    /// Shared news process configuration consumed by both seeded and coarse simulation.
    pub news_process_configuration: NewsProcessConfiguration,

    /// Excitation state carried into the first simulated interval.
    pub initial_excitation_state: HawkesExcitationState,

    /// Interval-aligned hourly factors (`s_h`, `nu_h`) used by the simulator.
    pub hourly_factor_contract: HourlyFactorContractOutput,

    /// Canonical calendar registry used to resolve open and closed market states.
    pub calendar_registry: ExchangeCalendarRegistry,
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

        self.model_inputs
            .nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .map_err(|source| {
                SeededTickSimulationRunError::InvalidNigSharedPlusShiftConfiguration { source }
            })?;
        self.model_inputs
            .news_process_configuration
            .validate()
            .map_err(|source| {
                SeededTickSimulationRunError::InvalidNewsProcessConfiguration { source }
            })?;
        validate_known_scheduled_news_timestamps(
            &self
                .model_inputs
                .news_process_configuration
                .arrivals
                .known
                .scheduled_event_timestamps_utc_nanos,
        )?;

        if self.options.debug_trace_enabled {
            self.options.debug_trace_options.validate()?;
        }

        Ok(())
    }
}
