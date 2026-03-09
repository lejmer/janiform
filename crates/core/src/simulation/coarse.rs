//! Coarse-interval simulation workflow contracts and time-indexed path artifacts.

use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::hawkes::HawkesExcitationState;
use crate::risk::{
    SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
    SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY,
    SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY,
    SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY, SufficientStatisticsIntervalStatistics,
    SufficientStatisticsIntervalStatisticsError, SufficientStatisticsRiskIntervalFactors,
    SufficientStatisticsRiskIntervalFactorsError, SufficientStatisticsRiskModelInputs,
    SufficientStatisticsRiskModelInputsError, SufficientStatisticsRiskReturnComponents,
    SufficientStatisticsRiskReturnComponentsError,
};

const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;
const ONE_SECOND_NANOS: i64 = 1_000_000_000;
const INTERVAL_DELTA_NANOS_TOLERANCE: f64 = 0.5;

pub const COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION: &str =
    "simulation.coarse_interval.run_request.v1";
pub const COARSE_SIMULATION_PATH_SCHEMA_VERSION: &str = "simulation.coarse_interval.path.v1";
pub const COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION: &str =
    "simulation.coarse_interval.path_record.v1";
pub const COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "simulation.coarse_interval.path_output_schema_metadata.v1";
pub const COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION: &str =
    "simulation.coarse_interval.path_provenance.v1";
pub const COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION: &str =
    "simulation.coarse_interval.path_diagnostics.v1";
pub const COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY: &str =
    "time_indexed_coarse_interval_simulation_path_for_validation_and_analytics.v1";
pub const COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY: &str =
    "exclude_environment_specific_metadata.v1";
pub const COARSE_SIMULATION_INTERVAL_TIMING_POLICY: &str =
    "start_time_utc_anchor_plus_fixed_interval_delta_half_open_utc_boundaries.v1";
pub const COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY: &str =
    "interval_index_and_explicit_utc_boundary_timestamps.v1";
pub const COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY: &str = "factor_row_zero_anchors_to_start_time_utc_and_rows_progress_in_request_interval_order_without_implicit_resampling.v1";
pub const COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY: &str =
    "interval_delta_must_map_to_an_exact_fixed_width_utc_nanosecond_grid.v1";

const COARSE_SIMULATION_PATH_REQUIRED_FIELDS: [&str; 13] = [
    "schema_version",
    "run_request_schema_version",
    "output_schema_metadata",
    "provenance",
    "diagnostics",
    "seed",
    "start_time_utc_nanos",
    "end_time_utc_nanos",
    "interval_delta",
    "simulated_interval_count",
    "initial_excitation_state",
    "terminal_excitation_state",
    "records",
];
const COARSE_SIMULATION_PATH_RECORD_REQUIRED_FIELDS: [&str; 6] = [
    "schema_version",
    "interval_index",
    "interval_start_utc_nanos",
    "interval_end_utc_nanos",
    "sufficient_statistics",
    "return_components",
];
const COARSE_SIMULATION_PATH_REQUIRED_PROVENANCE_FIELDS: [&str; 12] = [
    "schema_version",
    "artifact_workflow_policy",
    "environment_metadata_policy",
    "interval_timing_policy",
    "interval_identifier_policy",
    "interval_factor_alignment_policy",
    "compatible_interval_grid_policy",
    "interval_factor_policy",
    "transform_slice_sampling_policy",
    "boundary_state_update_policy",
    "interval_return_generation_policy",
    "interval_return_component_routing_policy",
];
const COARSE_SIMULATION_PATH_REQUIRED_DIAGNOSTICS_FIELDS: [&str; 15] = [
    "schema_version",
    "simulated_interval_count",
    "first_interval_index",
    "last_interval_index",
    "first_interval_start_utc_nanos",
    "last_interval_end_utc_nanos",
    "cumulative_a_plus",
    "cumulative_a_minus",
    "cumulative_n_plus",
    "cumulative_n_minus",
    "cumulative_trading_time_return",
    "cumulative_open_jump_return",
    "cumulative_closed_gap_return",
    "cumulative_total_return",
    "max_abs_total_return",
];

/// Typed coarse-interval simulation run request anchored to a real UTC start timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseSimulationRunRequest {
    pub schema_version: String,
    pub seed: u64,
    pub start_time_utc_nanos: i64,
    pub horizon_interval_count: usize,
    pub interval_delta: f64,
    pub model_inputs: SufficientStatisticsRiskModelInputs,
    pub interval_factors: SufficientStatisticsRiskIntervalFactors,
}

impl CoarseSimulationRunRequest {
    pub fn try_new(
        seed: u64,
        start_time_utc_nanos: i64,
        horizon_interval_count: usize,
        interval_delta: f64,
        model_inputs: SufficientStatisticsRiskModelInputs,
        interval_factors: SufficientStatisticsRiskIntervalFactors,
    ) -> Result<Self, CoarseSimulationRunRequestError> {
        let request = Self {
            schema_version: COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION.to_string(),
            seed,
            start_time_utc_nanos,
            horizon_interval_count,
            interval_delta,
            model_inputs,
            interval_factors,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), CoarseSimulationRunRequestError> {
        validate_exact_string(
            "schema_version",
            &self.schema_version,
            COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
        )
        .map_err(
            |source| CoarseSimulationRunRequestError::InvalidSchemaVersion {
                field_path: source.field_path,
                expected: source.expected,
                actual: source.actual,
            },
        )?;

        if utc_timestamp_from_nanos(self.start_time_utc_nanos).is_none() {
            return Err(CoarseSimulationRunRequestError::InvalidStartTimeUtcNanos {
                value: self.start_time_utc_nanos,
            });
        }

        if self.horizon_interval_count == 0 {
            return Err(CoarseSimulationRunRequestError::InvalidHorizonIntervalCount { value: 0 });
        }

        let interval_delta_nanos = interval_delta_to_nanos(self.interval_delta)
            .map_err(|source| CoarseSimulationRunRequestError::InvalidIntervalDelta { source })?;
        validate_horizon_end_timestamp(
            self.start_time_utc_nanos,
            interval_delta_nanos,
            self.horizon_interval_count,
        )
        .map_err(|source| CoarseSimulationRunRequestError::InvalidHorizonTimeIndex { source })?;

        self.model_inputs
            .validate()
            .map_err(|source| CoarseSimulationRunRequestError::InvalidModelInputs { source })?;

        self.interval_factors
            .validate()
            .map_err(|source| CoarseSimulationRunRequestError::InvalidIntervalFactors { source })?;

        let resolved_factor_interval_delta = self.interval_factors.resolved_interval_delta();
        if self.interval_delta != resolved_factor_interval_delta {
            return Err(
                CoarseSimulationRunRequestError::IntervalDeltaFactorMismatch {
                    requested_interval_delta: self.interval_delta,
                    available_factor_interval_delta: resolved_factor_interval_delta,
                },
            );
        }

        let available_interval_count = self.interval_factors.s_h.len();
        if self.horizon_interval_count > available_interval_count {
            return Err(
                CoarseSimulationRunRequestError::HorizonExceedsAvailableIntervals {
                    requested_interval_count: self.horizon_interval_count,
                    available_interval_count,
                },
            );
        }

        Ok(())
    }

    pub fn interval_delta_nanos(&self) -> Result<i64, IntervalDeltaGridError> {
        interval_delta_to_nanos(self.interval_delta)
    }

    pub fn end_time_utc_nanos(&self) -> Result<i64, CoarseSimulationTimeIndexError> {
        let interval_delta_nanos = interval_delta_to_nanos(self.interval_delta)
            .map_err(|source| CoarseSimulationTimeIndexError::InvalidIntervalDelta { source })?;
        compute_horizon_end_time_utc_nanos(
            self.start_time_utc_nanos,
            interval_delta_nanos,
            self.horizon_interval_count,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationRunRequestError {
    InvalidSchemaVersion {
        field_path: &'static str,
        expected: String,
        actual: String,
    },
    InvalidStartTimeUtcNanos {
        value: i64,
    },
    InvalidHorizonIntervalCount {
        value: usize,
    },
    InvalidIntervalDelta {
        source: IntervalDeltaGridError,
    },
    InvalidHorizonTimeIndex {
        source: CoarseSimulationTimeIndexError,
    },
    InvalidModelInputs {
        source: SufficientStatisticsRiskModelInputsError,
    },
    InvalidIntervalFactors {
        source: SufficientStatisticsRiskIntervalFactorsError,
    },
    IntervalDeltaFactorMismatch {
        requested_interval_delta: f64,
        available_factor_interval_delta: f64,
    },
    HorizonExceedsAvailableIntervals {
        requested_interval_count: usize,
        available_interval_count: usize,
    },
}

impl Display for CoarseSimulationRunRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion {
                field_path,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_path} must be {expected} for the coarse simulation run request contract, found {actual}"
            ),
            Self::InvalidStartTimeUtcNanos { value } => write!(
                formatter,
                "start_time_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
            Self::InvalidHorizonIntervalCount { value } => write!(
                formatter,
                "horizon_interval_count must be strictly positive, found {value}"
            ),
            Self::InvalidIntervalDelta { source } => {
                write!(formatter, "interval_delta is invalid: {source}")
            }
            Self::InvalidHorizonTimeIndex { source } => {
                write!(
                    formatter,
                    "time-indexed horizon construction is invalid: {source}"
                )
            }
            Self::InvalidModelInputs { source } => {
                write!(formatter, "model_inputs payload is invalid: {source}")
            }
            Self::InvalidIntervalFactors { source } => {
                write!(formatter, "interval_factors payload is invalid: {source}")
            }
            Self::IntervalDeltaFactorMismatch {
                requested_interval_delta,
                available_factor_interval_delta,
            } => write!(
                formatter,
                "interval_delta ({requested_interval_delta}) must match interval_factors.interval_delta ({available_factor_interval_delta}) under the coarse simulation factor-alignment policy"
            ),
            Self::HorizonExceedsAvailableIntervals {
                requested_interval_count,
                available_interval_count,
            } => write!(
                formatter,
                "horizon_interval_count ({requested_interval_count}) exceeds available interval_factors entries ({available_interval_count})"
            ),
        }
    }
}

impl Error for CoarseSimulationRunRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidIntervalDelta { source } => Some(source),
            Self::InvalidHorizonTimeIndex { source } => Some(source),
            Self::InvalidModelInputs { source } => Some(source),
            Self::InvalidIntervalFactors { source } => Some(source),
            _ => None,
        }
    }
}

/// One time-indexed interval record in a coarse simulation path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseSimulationPathRecord {
    pub schema_version: String,
    pub interval_index: usize,
    pub interval_start_utc_nanos: i64,
    pub interval_end_utc_nanos: i64,
    pub sufficient_statistics: SufficientStatisticsIntervalStatistics,
    pub return_components: SufficientStatisticsRiskReturnComponents,
}

impl CoarseSimulationPathRecord {
    pub fn try_new(
        interval_index: usize,
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
        sufficient_statistics: SufficientStatisticsIntervalStatistics,
        return_components: SufficientStatisticsRiskReturnComponents,
    ) -> Result<Self, CoarseSimulationPathRecordError> {
        let record = Self {
            schema_version: COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION.to_string(),
            interval_index,
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            sufficient_statistics,
            return_components,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), CoarseSimulationPathRecordError> {
        validate_exact_string(
            "schema_version",
            &self.schema_version,
            COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION,
        )
        .map_err(CoarseSimulationPathRecordError::InvalidSchemaVersion)?;

        validate_interval_bounds(self.interval_start_utc_nanos, self.interval_end_utc_nanos)
            .map_err(CoarseSimulationPathRecordError::InvalidIntervalBounds)?;

        self.sufficient_statistics.validate().map_err(|source| {
            CoarseSimulationPathRecordError::InvalidSufficientStatistics { source }
        })?;
        self.return_components.validate().map_err(|source| {
            CoarseSimulationPathRecordError::InvalidReturnComponents { source }
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationPathRecordError {
    InvalidSchemaVersion(ExactStringValidationError),
    InvalidIntervalBounds(CoarseSimulationIntervalBoundsError),
    InvalidSufficientStatistics {
        source: SufficientStatisticsIntervalStatisticsError,
    },
    InvalidReturnComponents {
        source: SufficientStatisticsRiskReturnComponentsError,
    },
}

impl Display for CoarseSimulationPathRecordError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion(source) => {
                write!(formatter, "schema_version is invalid: {source}")
            }
            Self::InvalidIntervalBounds(source) => {
                write!(
                    formatter,
                    "interval boundary timestamps are invalid: {source}"
                )
            }
            Self::InvalidSufficientStatistics { source } => {
                write!(formatter, "sufficient_statistics is invalid: {source}")
            }
            Self::InvalidReturnComponents { source } => {
                write!(formatter, "return_components is invalid: {source}")
            }
        }
    }
}

impl Error for CoarseSimulationPathRecordError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSchemaVersion(source) => Some(source),
            Self::InvalidIntervalBounds(source) => Some(source),
            Self::InvalidSufficientStatistics { source } => Some(source),
            Self::InvalidReturnComponents { source } => Some(source),
        }
    }
}

/// Machine-readable metadata for coarse path artifacts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseSimulationPathOutputSchemaMetadata {
    pub schema_version: String,
    pub provenance_schema_version: String,
    pub diagnostics_schema_version: String,
    pub artifact_workflow_policy: String,
    pub environment_metadata_policy: String,
    pub required_path_fields: Vec<String>,
    pub required_record_fields: Vec<String>,
    pub required_provenance_fields: Vec<String>,
    pub required_diagnostics_fields: Vec<String>,
}

/// Deterministic provenance payload for coarse path artifacts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseSimulationPathProvenance {
    pub schema_version: String,
    pub artifact_workflow_policy: String,
    pub environment_metadata_policy: String,
    pub interval_timing_policy: String,
    pub interval_identifier_policy: String,
    pub interval_factor_alignment_policy: String,
    pub compatible_interval_grid_policy: String,
    pub interval_factor_policy: String,
    pub transform_slice_sampling_policy: String,
    pub boundary_state_update_policy: String,
    pub interval_return_generation_policy: String,
    pub interval_return_component_routing_policy: String,
    pub batch_seed_partition_policy: String,
}

/// Concise deterministic diagnostics for coarse simulation path validation and analytics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseSimulationPathDiagnostics {
    pub schema_version: String,
    pub simulated_interval_count: usize,
    pub first_interval_index: usize,
    pub last_interval_index: usize,
    pub first_interval_start_utc_nanos: i64,
    pub last_interval_end_utc_nanos: i64,
    pub cumulative_a_plus: f64,
    pub cumulative_a_minus: f64,
    pub cumulative_n_plus: u64,
    pub cumulative_n_minus: u64,
    pub cumulative_trading_time_return: f64,
    pub cumulative_open_jump_return: f64,
    pub cumulative_closed_gap_return: f64,
    pub cumulative_total_return: f64,
    pub max_abs_total_return: f64,
}

/// Canonical time-indexed coarse simulation path artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseSimulationPath {
    pub schema_version: String,
    pub run_request_schema_version: String,
    pub output_schema_metadata: CoarseSimulationPathOutputSchemaMetadata,
    pub provenance: CoarseSimulationPathProvenance,
    pub diagnostics: CoarseSimulationPathDiagnostics,
    pub seed: u64,
    pub start_time_utc_nanos: i64,
    pub end_time_utc_nanos: i64,
    pub interval_delta: f64,
    pub simulated_interval_count: usize,
    pub initial_excitation_state: HawkesExcitationState,
    pub terminal_excitation_state: HawkesExcitationState,
    pub records: Vec<CoarseSimulationPathRecord>,
}

impl CoarseSimulationPath {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        seed: u64,
        start_time_utc_nanos: i64,
        interval_delta: f64,
        simulated_interval_count: usize,
        initial_excitation_state: HawkesExcitationState,
        terminal_excitation_state: HawkesExcitationState,
        records: Vec<CoarseSimulationPathRecord>,
    ) -> Result<Self, CoarseSimulationPathError> {
        let end_time_utc_nanos = derive_path_end_time_utc_nanos(
            start_time_utc_nanos,
            interval_delta,
            simulated_interval_count,
            &records,
        )?;
        let diagnostics =
            derive_coarse_simulation_path_diagnostics(simulated_interval_count, &records)?;

        let path = Self {
            schema_version: COARSE_SIMULATION_PATH_SCHEMA_VERSION.to_string(),
            run_request_schema_version: COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION.to_string(),
            output_schema_metadata: build_coarse_simulation_path_output_schema_metadata(),
            provenance: build_coarse_simulation_path_provenance(),
            diagnostics,
            seed,
            start_time_utc_nanos,
            end_time_utc_nanos,
            interval_delta,
            simulated_interval_count,
            initial_excitation_state,
            terminal_excitation_state,
            records,
        };

        path.validate()?;

        Ok(path)
    }

    pub fn validate(&self) -> Result<(), CoarseSimulationPathError> {
        validate_exact_string(
            "schema_version",
            &self.schema_version,
            COARSE_SIMULATION_PATH_SCHEMA_VERSION,
        )
        .map_err(CoarseSimulationPathError::InvalidSchemaVersion)?;
        validate_exact_string(
            "run_request_schema_version",
            &self.run_request_schema_version,
            COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
        )
        .map_err(CoarseSimulationPathError::InvalidSchemaVersion)?;

        if utc_timestamp_from_nanos(self.start_time_utc_nanos).is_none() {
            return Err(CoarseSimulationPathError::InvalidStartTimeUtcNanos {
                value: self.start_time_utc_nanos,
            });
        }
        if utc_timestamp_from_nanos(self.end_time_utc_nanos).is_none() {
            return Err(CoarseSimulationPathError::InvalidEndTimeUtcNanos {
                value: self.end_time_utc_nanos,
            });
        }
        if self.simulated_interval_count == 0 {
            return Err(CoarseSimulationPathError::InvalidSimulatedIntervalCount { value: 0 });
        }

        let interval_delta_nanos = interval_delta_to_nanos(self.interval_delta)
            .map_err(|source| CoarseSimulationPathError::InvalidIntervalDelta { source })?;

        if self.records.len() != self.simulated_interval_count {
            return Err(CoarseSimulationPathError::RecordCountMismatch {
                simulated_interval_count: self.simulated_interval_count,
                record_count: self.records.len(),
            });
        }

        validate_excitation_state(self.initial_excitation_state).map_err(|(field, value)| {
            CoarseSimulationPathError::InvalidInitialState { field, value }
        })?;
        validate_excitation_state(self.terminal_excitation_state).map_err(|(field, value)| {
            CoarseSimulationPathError::InvalidTerminalState { field, value }
        })?;
        validate_output_schema_metadata(&self.output_schema_metadata)?;
        validate_provenance(&self.provenance)?;

        for (record_position, record) in self.records.iter().enumerate() {
            record
                .validate()
                .map_err(|source| CoarseSimulationPathError::InvalidRecord {
                    record_position,
                    source,
                })?;

            if record.interval_index != record_position {
                return Err(CoarseSimulationPathError::NonSequentialIntervalIndex {
                    record_position,
                    record_interval_index: record.interval_index,
                });
            }

            let expected_interval_start_utc_nanos = compute_interval_start_utc_nanos(
                self.start_time_utc_nanos,
                interval_delta_nanos,
                record_position,
            )
            .map_err(|source| CoarseSimulationPathError::InvalidTimeIndex { source })?;
            let expected_interval_end_utc_nanos = expected_interval_start_utc_nanos
                .checked_add(interval_delta_nanos)
                .ok_or(CoarseSimulationPathError::InvalidTimeIndex {
                    source: CoarseSimulationTimeIndexError::EndTimeOverflow {
                        start_time_utc_nanos: expected_interval_start_utc_nanos,
                        interval_delta_nanos,
                        horizon_interval_count: 1,
                    },
                })?;

            if record.interval_start_utc_nanos != expected_interval_start_utc_nanos {
                return Err(CoarseSimulationPathError::IntervalBoundaryMismatch {
                    record_position,
                    field_path: "interval_start_utc_nanos",
                    expected: expected_interval_start_utc_nanos,
                    actual: record.interval_start_utc_nanos,
                });
            }
            if record.interval_end_utc_nanos != expected_interval_end_utc_nanos {
                return Err(CoarseSimulationPathError::IntervalBoundaryMismatch {
                    record_position,
                    field_path: "interval_end_utc_nanos",
                    expected: expected_interval_end_utc_nanos,
                    actual: record.interval_end_utc_nanos,
                });
            }
        }

        let expected_end_time_utc_nanos = compute_horizon_end_time_utc_nanos(
            self.start_time_utc_nanos,
            interval_delta_nanos,
            self.simulated_interval_count,
        )
        .map_err(|source| CoarseSimulationPathError::InvalidTimeIndex { source })?;
        if self.end_time_utc_nanos != expected_end_time_utc_nanos {
            return Err(CoarseSimulationPathError::InvalidEndTimeAlignment {
                expected_end_time_utc_nanos,
                actual_end_time_utc_nanos: self.end_time_utc_nanos,
            });
        }

        let expected_diagnostics = derive_coarse_simulation_path_diagnostics(
            self.simulated_interval_count,
            &self.records,
        )?;

        validate_diagnostics(&self.diagnostics, &expected_diagnostics)?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationPathError {
    InvalidSchemaVersion(ExactStringValidationError),
    InvalidStartTimeUtcNanos {
        value: i64,
    },
    InvalidEndTimeUtcNanos {
        value: i64,
    },
    InvalidIntervalDelta {
        source: IntervalDeltaGridError,
    },
    InvalidSimulatedIntervalCount {
        value: usize,
    },
    RecordCountMismatch {
        simulated_interval_count: usize,
        record_count: usize,
    },
    InvalidInitialState {
        field: &'static str,
        value: f64,
    },
    InvalidTerminalState {
        field: &'static str,
        value: f64,
    },
    InvalidRecord {
        record_position: usize,
        source: CoarseSimulationPathRecordError,
    },
    NonSequentialIntervalIndex {
        record_position: usize,
        record_interval_index: usize,
    },
    IntervalBoundaryMismatch {
        record_position: usize,
        field_path: &'static str,
        expected: i64,
        actual: i64,
    },
    InvalidTimeIndex {
        source: CoarseSimulationTimeIndexError,
    },
    InvalidEndTimeAlignment {
        expected_end_time_utc_nanos: i64,
        actual_end_time_utc_nanos: i64,
    },
    InvalidOutputSchemaMetadata {
        field_path: &'static str,
        expected: String,
        actual: String,
    },
    InvalidProvenance {
        field_path: &'static str,
        expected: String,
        actual: String,
    },
    InvalidDiagnostics {
        field_path: &'static str,
        expected: String,
        actual: String,
    },
}

impl Display for CoarseSimulationPathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion(source) => {
                write!(formatter, "schema version field is invalid: {source}")
            }
            Self::InvalidStartTimeUtcNanos { value } => write!(
                formatter,
                "start_time_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
            Self::InvalidEndTimeUtcNanos { value } => write!(
                formatter,
                "end_time_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
            Self::InvalidIntervalDelta { source } => {
                write!(formatter, "interval_delta is invalid: {source}")
            }
            Self::InvalidSimulatedIntervalCount { value } => write!(
                formatter,
                "simulated_interval_count must be strictly positive, found {value}"
            ),
            Self::RecordCountMismatch {
                simulated_interval_count,
                record_count,
            } => write!(
                formatter,
                "simulated_interval_count ({simulated_interval_count}) must match records length ({record_count})"
            ),
            Self::InvalidInitialState { field, value } => write!(
                formatter,
                "initial_excitation_state.{field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidTerminalState { field, value } => write!(
                formatter,
                "terminal_excitation_state.{field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidRecord {
                record_position,
                source,
            } => write!(formatter, "records[{record_position}] is invalid: {source}"),
            Self::NonSequentialIntervalIndex {
                record_position,
                record_interval_index,
            } => write!(
                formatter,
                "records[{record_position}].interval_index must equal {record_position}, found {record_interval_index}"
            ),
            Self::IntervalBoundaryMismatch {
                record_position,
                field_path,
                expected,
                actual,
            } => write!(
                formatter,
                "records[{record_position}].{field_path} must equal {expected}, found {actual}"
            ),
            Self::InvalidTimeIndex { source } => {
                write!(formatter, "path time index is invalid: {source}")
            }
            Self::InvalidEndTimeAlignment {
                expected_end_time_utc_nanos,
                actual_end_time_utc_nanos,
            } => write!(
                formatter,
                "end_time_utc_nanos must equal the final interval end ({expected_end_time_utc_nanos}), found {actual_end_time_utc_nanos}"
            ),
            Self::InvalidOutputSchemaMetadata {
                field_path,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_path} must be {expected} for deterministic coarse path output metadata, found {actual}"
            ),
            Self::InvalidProvenance {
                field_path,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_path} must be {expected} for deterministic coarse path provenance, found {actual}"
            ),
            Self::InvalidDiagnostics {
                field_path,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_path} must be {expected} for deterministic coarse path diagnostics, found {actual}"
            ),
        }
    }
}

impl Error for CoarseSimulationPathError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSchemaVersion(source) => Some(source),
            Self::InvalidIntervalDelta { source } => Some(source),
            Self::InvalidRecord { source, .. } => Some(source),
            Self::InvalidTimeIndex { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntervalDeltaGridError {
    NonPositiveOrNonFinite {
        value: f64,
    },
    NotRepresentableAsUtcNanoseconds {
        interval_delta: f64,
        implied_interval_duration_nanos: f64,
    },
}

impl Display for IntervalDeltaGridError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonPositiveOrNonFinite { value } => write!(
                formatter,
                "interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::NotRepresentableAsUtcNanoseconds {
                interval_delta,
                implied_interval_duration_nanos,
            } => write!(
                formatter,
                "interval_delta ({interval_delta}) must map to an exact fixed-width UTC nanosecond duration, found implied duration {implied_interval_duration_nanos}"
            ),
        }
    }
}

impl Error for IntervalDeltaGridError {}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationTimeIndexError {
    InvalidIntervalDelta {
        source: IntervalDeltaGridError,
    },
    IntervalCountOverflow {
        value: usize,
    },
    EndTimeOverflow {
        start_time_utc_nanos: i64,
        interval_delta_nanos: i64,
        horizon_interval_count: usize,
    },
    InvalidEndTimeUtcNanos {
        value: i64,
    },
}

impl Display for CoarseSimulationTimeIndexError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIntervalDelta { source } => {
                write!(formatter, "interval_delta is invalid: {source}")
            }
            Self::IntervalCountOverflow { value } => write!(
                formatter,
                "horizon_interval_count ({value}) cannot be represented in the coarse UTC interval grid"
            ),
            Self::EndTimeOverflow {
                start_time_utc_nanos,
                interval_delta_nanos,
                horizon_interval_count,
            } => write!(
                formatter,
                "start_time_utc_nanos ({start_time_utc_nanos}) with interval_delta_nanos ({interval_delta_nanos}) and horizon_interval_count ({horizon_interval_count}) overflowed the UTC time index"
            ),
            Self::InvalidEndTimeUtcNanos { value } => write!(
                formatter,
                "derived end_time_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
        }
    }
}

impl Error for CoarseSimulationTimeIndexError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidIntervalDelta { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationIntervalBoundsError {
    InvalidStartTimeUtcNanos {
        value: i64,
    },
    InvalidEndTimeUtcNanos {
        value: i64,
    },
    NonIncreasingBounds {
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    },
}

impl Display for CoarseSimulationIntervalBoundsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStartTimeUtcNanos { value } => write!(
                formatter,
                "interval_start_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
            Self::InvalidEndTimeUtcNanos { value } => write!(
                formatter,
                "interval_end_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
            Self::NonIncreasingBounds {
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            } => write!(
                formatter,
                "interval_start_utc_nanos ({interval_start_utc_nanos}) must be strictly smaller than interval_end_utc_nanos ({interval_end_utc_nanos})"
            ),
        }
    }
}

impl Error for CoarseSimulationIntervalBoundsError {}

#[derive(Debug, Clone, PartialEq)]
pub struct ExactStringValidationError {
    field_path: &'static str,
    expected: String,
    actual: String,
}

impl Display for ExactStringValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} must be {}, found {}",
            self.field_path, self.expected, self.actual
        )
    }
}

impl Error for ExactStringValidationError {}

fn interval_delta_to_nanos(interval_delta: f64) -> Result<i64, IntervalDeltaGridError> {
    if !interval_delta.is_finite() || interval_delta <= 0.0 {
        return Err(IntervalDeltaGridError::NonPositiveOrNonFinite {
            value: interval_delta,
        });
    }

    let implied_interval_duration_nanos = interval_delta * ONE_HOUR_NANOS as f64;
    let rounded_interval_duration_nanos = implied_interval_duration_nanos.round();
    if !implied_interval_duration_nanos.is_finite()
        || rounded_interval_duration_nanos <= 0.0
        || (implied_interval_duration_nanos - rounded_interval_duration_nanos).abs()
            > INTERVAL_DELTA_NANOS_TOLERANCE
    {
        return Err(IntervalDeltaGridError::NotRepresentableAsUtcNanoseconds {
            interval_delta,
            implied_interval_duration_nanos,
        });
    }

    Ok(rounded_interval_duration_nanos as i64)
}

fn validate_horizon_end_timestamp(
    start_time_utc_nanos: i64,
    interval_delta_nanos: i64,
    horizon_interval_count: usize,
) -> Result<(), CoarseSimulationTimeIndexError> {
    compute_horizon_end_time_utc_nanos(
        start_time_utc_nanos,
        interval_delta_nanos,
        horizon_interval_count,
    )?;
    Ok(())
}

fn compute_horizon_end_time_utc_nanos(
    start_time_utc_nanos: i64,
    interval_delta_nanos: i64,
    horizon_interval_count: usize,
) -> Result<i64, CoarseSimulationTimeIndexError> {
    let interval_count_i64 = i64::try_from(horizon_interval_count).map_err(|_| {
        CoarseSimulationTimeIndexError::IntervalCountOverflow {
            value: horizon_interval_count,
        }
    })?;
    let total_duration_nanos = interval_delta_nanos.checked_mul(interval_count_i64).ok_or(
        CoarseSimulationTimeIndexError::EndTimeOverflow {
            start_time_utc_nanos,
            interval_delta_nanos,
            horizon_interval_count,
        },
    )?;
    let end_time_utc_nanos = start_time_utc_nanos
        .checked_add(total_duration_nanos)
        .ok_or(CoarseSimulationTimeIndexError::EndTimeOverflow {
            start_time_utc_nanos,
            interval_delta_nanos,
            horizon_interval_count,
        })?;

    if utc_timestamp_from_nanos(end_time_utc_nanos).is_none() {
        return Err(CoarseSimulationTimeIndexError::InvalidEndTimeUtcNanos {
            value: end_time_utc_nanos,
        });
    }

    Ok(end_time_utc_nanos)
}

fn compute_interval_start_utc_nanos(
    start_time_utc_nanos: i64,
    interval_delta_nanos: i64,
    interval_index: usize,
) -> Result<i64, CoarseSimulationTimeIndexError> {
    compute_horizon_end_time_utc_nanos(start_time_utc_nanos, interval_delta_nanos, interval_index)
}

fn validate_interval_bounds(
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
) -> Result<(), CoarseSimulationIntervalBoundsError> {
    if utc_timestamp_from_nanos(interval_start_utc_nanos).is_none() {
        return Err(
            CoarseSimulationIntervalBoundsError::InvalidStartTimeUtcNanos {
                value: interval_start_utc_nanos,
            },
        );
    }
    if utc_timestamp_from_nanos(interval_end_utc_nanos).is_none() {
        return Err(
            CoarseSimulationIntervalBoundsError::InvalidEndTimeUtcNanos {
                value: interval_end_utc_nanos,
            },
        );
    }
    if interval_end_utc_nanos <= interval_start_utc_nanos {
        return Err(CoarseSimulationIntervalBoundsError::NonIncreasingBounds {
            interval_start_utc_nanos,
            interval_end_utc_nanos,
        });
    }

    Ok(())
}

fn validate_exact_string(
    field_path: &'static str,
    actual: &str,
    expected: &'static str,
) -> Result<(), ExactStringValidationError> {
    if actual != expected {
        return Err(ExactStringValidationError {
            field_path,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }

    Ok(())
}

fn validate_excitation_state(state: HawkesExcitationState) -> Result<(), (&'static str, f64)> {
    let fields = [
        ("y_plus_plus", state.y_plus_plus),
        ("y_plus_minus", state.y_plus_minus),
        ("y_minus_plus", state.y_minus_plus),
        ("y_minus_minus", state.y_minus_minus),
    ];

    for (field, value) in fields {
        if !value.is_finite() || value < 0.0 {
            return Err((field, value));
        }
    }

    Ok(())
}

fn utc_timestamp_from_nanos(timestamp_utc_nanos: i64) -> Option<DateTime<Utc>> {
    let seconds = timestamp_utc_nanos.div_euclid(ONE_SECOND_NANOS);
    let subsecond_nanos = timestamp_utc_nanos.rem_euclid(ONE_SECOND_NANOS) as u32;

    DateTime::from_timestamp(seconds, subsecond_nanos)
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn build_string_list(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn build_coarse_simulation_path_output_schema_metadata() -> CoarseSimulationPathOutputSchemaMetadata
{
    CoarseSimulationPathOutputSchemaMetadata {
        schema_version: COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION.to_string(),
        provenance_schema_version: COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION.to_string(),
        diagnostics_schema_version: COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION.to_string(),
        artifact_workflow_policy: COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY.to_string(),
        environment_metadata_policy: COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY.to_string(),
        required_path_fields: build_string_list(&COARSE_SIMULATION_PATH_REQUIRED_FIELDS),
        required_record_fields: build_string_list(&COARSE_SIMULATION_PATH_RECORD_REQUIRED_FIELDS),
        required_provenance_fields: build_string_list(
            &COARSE_SIMULATION_PATH_REQUIRED_PROVENANCE_FIELDS,
        ),
        required_diagnostics_fields: build_string_list(
            &COARSE_SIMULATION_PATH_REQUIRED_DIAGNOSTICS_FIELDS,
        ),
    }
}

fn build_coarse_simulation_path_provenance() -> CoarseSimulationPathProvenance {
    CoarseSimulationPathProvenance {
        schema_version: COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION.to_string(),
        artifact_workflow_policy: COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY.to_string(),
        environment_metadata_policy: COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY.to_string(),
        interval_timing_policy: COARSE_SIMULATION_INTERVAL_TIMING_POLICY.to_string(),
        interval_identifier_policy: COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY.to_string(),
        interval_factor_alignment_policy: COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY
            .to_string(),
        compatible_interval_grid_policy: COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY
            .to_string(),
        interval_factor_policy: SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY.to_string(),
        transform_slice_sampling_policy: SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY
            .to_string(),
        boundary_state_update_policy: SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY
            .to_string(),
        interval_return_generation_policy: SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY
            .to_string(),
        interval_return_component_routing_policy:
            SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY.to_string(),
        batch_seed_partition_policy: SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY.to_string(),
    }
}

fn derive_coarse_simulation_path_diagnostics(
    simulated_interval_count: usize,
    records: &[CoarseSimulationPathRecord],
) -> Result<CoarseSimulationPathDiagnostics, CoarseSimulationPathError> {
    let first_record =
        records
            .first()
            .ok_or(CoarseSimulationPathError::InvalidSimulatedIntervalCount {
                value: simulated_interval_count,
            })?;
    let last_record =
        records
            .last()
            .ok_or(CoarseSimulationPathError::InvalidSimulatedIntervalCount {
                value: simulated_interval_count,
            })?;

    let mut cumulative_a_plus = 0.0;
    let mut cumulative_a_minus = 0.0;
    let mut cumulative_n_plus = 0_u64;
    let mut cumulative_n_minus = 0_u64;
    let mut cumulative_trading_time_return = 0.0;
    let mut cumulative_open_jump_return = 0.0;
    let mut cumulative_closed_gap_return = 0.0;
    let mut cumulative_total_return = 0.0;
    let mut max_abs_total_return: f64 = 0.0;

    for record in records {
        cumulative_a_plus += record.sufficient_statistics.a_plus;
        cumulative_a_minus += record.sufficient_statistics.a_minus;
        cumulative_n_plus = cumulative_n_plus
            .checked_add(record.sufficient_statistics.n_plus)
            .ok_or(CoarseSimulationPathError::InvalidDiagnostics {
                field_path: "diagnostics.cumulative_n_plus",
                expected: "finite cumulative count".to_string(),
                actual: "overflow".to_string(),
            })?;
        cumulative_n_minus = cumulative_n_minus
            .checked_add(record.sufficient_statistics.n_minus)
            .ok_or(CoarseSimulationPathError::InvalidDiagnostics {
                field_path: "diagnostics.cumulative_n_minus",
                expected: "finite cumulative count".to_string(),
                actual: "overflow".to_string(),
            })?;
        cumulative_trading_time_return += record.return_components.trading_time_return;
        cumulative_open_jump_return += record.return_components.open_jump_return;
        cumulative_closed_gap_return += record.return_components.closed_gap_return;
        cumulative_total_return += record.return_components.total_return;
        max_abs_total_return =
            max_abs_total_return.max(record.return_components.total_return.abs());
    }

    Ok(CoarseSimulationPathDiagnostics {
        schema_version: COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION.to_string(),
        simulated_interval_count,
        first_interval_index: first_record.interval_index,
        last_interval_index: last_record.interval_index,
        first_interval_start_utc_nanos: first_record.interval_start_utc_nanos,
        last_interval_end_utc_nanos: last_record.interval_end_utc_nanos,
        cumulative_a_plus,
        cumulative_a_minus,
        cumulative_n_plus,
        cumulative_n_minus,
        cumulative_trading_time_return,
        cumulative_open_jump_return,
        cumulative_closed_gap_return,
        cumulative_total_return,
        max_abs_total_return,
    })
}

fn derive_path_end_time_utc_nanos(
    start_time_utc_nanos: i64,
    interval_delta: f64,
    simulated_interval_count: usize,
    records: &[CoarseSimulationPathRecord],
) -> Result<i64, CoarseSimulationPathError> {
    let interval_delta_nanos = interval_delta_to_nanos(interval_delta)
        .map_err(|source| CoarseSimulationPathError::InvalidIntervalDelta { source })?;
    if simulated_interval_count == 0 {
        return Err(CoarseSimulationPathError::InvalidSimulatedIntervalCount { value: 0 });
    }
    if records.len() != simulated_interval_count {
        return Err(CoarseSimulationPathError::RecordCountMismatch {
            simulated_interval_count,
            record_count: records.len(),
        });
    }

    let expected_end_time_utc_nanos = compute_horizon_end_time_utc_nanos(
        start_time_utc_nanos,
        interval_delta_nanos,
        simulated_interval_count,
    )
    .map_err(|source| CoarseSimulationPathError::InvalidTimeIndex { source })?;

    if records[simulated_interval_count - 1].interval_end_utc_nanos != expected_end_time_utc_nanos {
        return Err(CoarseSimulationPathError::InvalidEndTimeAlignment {
            expected_end_time_utc_nanos,
            actual_end_time_utc_nanos: records[simulated_interval_count - 1].interval_end_utc_nanos,
        });
    }

    Ok(expected_end_time_utc_nanos)
}

fn validate_output_schema_metadata(
    metadata: &CoarseSimulationPathOutputSchemaMetadata,
) -> Result<(), CoarseSimulationPathError> {
    let expected = build_coarse_simulation_path_output_schema_metadata();

    validate_metadata_field(
        "output_schema_metadata.schema_version",
        &metadata.schema_version,
        &expected.schema_version,
    )?;
    validate_metadata_field(
        "output_schema_metadata.provenance_schema_version",
        &metadata.provenance_schema_version,
        &expected.provenance_schema_version,
    )?;
    validate_metadata_field(
        "output_schema_metadata.diagnostics_schema_version",
        &metadata.diagnostics_schema_version,
        &expected.diagnostics_schema_version,
    )?;
    validate_metadata_field(
        "output_schema_metadata.artifact_workflow_policy",
        &metadata.artifact_workflow_policy,
        &expected.artifact_workflow_policy,
    )?;
    validate_metadata_field(
        "output_schema_metadata.environment_metadata_policy",
        &metadata.environment_metadata_policy,
        &expected.environment_metadata_policy,
    )?;
    validate_metadata_list_field(
        "output_schema_metadata.required_path_fields",
        &metadata.required_path_fields,
        &expected.required_path_fields,
    )?;
    validate_metadata_list_field(
        "output_schema_metadata.required_record_fields",
        &metadata.required_record_fields,
        &expected.required_record_fields,
    )?;
    validate_metadata_list_field(
        "output_schema_metadata.required_provenance_fields",
        &metadata.required_provenance_fields,
        &expected.required_provenance_fields,
    )?;
    validate_metadata_list_field(
        "output_schema_metadata.required_diagnostics_fields",
        &metadata.required_diagnostics_fields,
        &expected.required_diagnostics_fields,
    )?;

    Ok(())
}

fn validate_provenance(
    provenance: &CoarseSimulationPathProvenance,
) -> Result<(), CoarseSimulationPathError> {
    let expected = build_coarse_simulation_path_provenance();

    validate_provenance_field(
        "provenance.schema_version",
        &provenance.schema_version,
        &expected.schema_version,
    )?;
    validate_provenance_field(
        "provenance.artifact_workflow_policy",
        &provenance.artifact_workflow_policy,
        &expected.artifact_workflow_policy,
    )?;
    validate_provenance_field(
        "provenance.environment_metadata_policy",
        &provenance.environment_metadata_policy,
        &expected.environment_metadata_policy,
    )?;
    validate_provenance_field(
        "provenance.interval_timing_policy",
        &provenance.interval_timing_policy,
        &expected.interval_timing_policy,
    )?;
    validate_provenance_field(
        "provenance.interval_identifier_policy",
        &provenance.interval_identifier_policy,
        &expected.interval_identifier_policy,
    )?;
    validate_provenance_field(
        "provenance.interval_factor_alignment_policy",
        &provenance.interval_factor_alignment_policy,
        &expected.interval_factor_alignment_policy,
    )?;
    validate_provenance_field(
        "provenance.compatible_interval_grid_policy",
        &provenance.compatible_interval_grid_policy,
        &expected.compatible_interval_grid_policy,
    )?;
    validate_provenance_field(
        "provenance.interval_factor_policy",
        &provenance.interval_factor_policy,
        &expected.interval_factor_policy,
    )?;
    validate_provenance_field(
        "provenance.transform_slice_sampling_policy",
        &provenance.transform_slice_sampling_policy,
        &expected.transform_slice_sampling_policy,
    )?;
    validate_provenance_field(
        "provenance.boundary_state_update_policy",
        &provenance.boundary_state_update_policy,
        &expected.boundary_state_update_policy,
    )?;
    validate_provenance_field(
        "provenance.interval_return_generation_policy",
        &provenance.interval_return_generation_policy,
        &expected.interval_return_generation_policy,
    )?;
    validate_provenance_field(
        "provenance.interval_return_component_routing_policy",
        &provenance.interval_return_component_routing_policy,
        &expected.interval_return_component_routing_policy,
    )?;
    validate_provenance_field(
        "provenance.batch_seed_partition_policy",
        &provenance.batch_seed_partition_policy,
        &expected.batch_seed_partition_policy,
    )?;

    Ok(())
}

fn validate_diagnostics(
    diagnostics: &CoarseSimulationPathDiagnostics,
    expected: &CoarseSimulationPathDiagnostics,
) -> Result<(), CoarseSimulationPathError> {
    validate_diagnostics_field(
        "diagnostics.schema_version",
        &diagnostics.schema_version,
        &expected.schema_version,
    )?;
    validate_diagnostics_field(
        "diagnostics.simulated_interval_count",
        &diagnostics.simulated_interval_count.to_string(),
        &expected.simulated_interval_count.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.first_interval_index",
        &diagnostics.first_interval_index.to_string(),
        &expected.first_interval_index.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.last_interval_index",
        &diagnostics.last_interval_index.to_string(),
        &expected.last_interval_index.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.first_interval_start_utc_nanos",
        &diagnostics.first_interval_start_utc_nanos.to_string(),
        &expected.first_interval_start_utc_nanos.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.last_interval_end_utc_nanos",
        &diagnostics.last_interval_end_utc_nanos.to_string(),
        &expected.last_interval_end_utc_nanos.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.cumulative_a_plus",
        &diagnostics.cumulative_a_plus.to_string(),
        &expected.cumulative_a_plus.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.cumulative_a_minus",
        &diagnostics.cumulative_a_minus.to_string(),
        &expected.cumulative_a_minus.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.cumulative_n_plus",
        &diagnostics.cumulative_n_plus.to_string(),
        &expected.cumulative_n_plus.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.cumulative_n_minus",
        &diagnostics.cumulative_n_minus.to_string(),
        &expected.cumulative_n_minus.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.cumulative_trading_time_return",
        &diagnostics.cumulative_trading_time_return.to_string(),
        &expected.cumulative_trading_time_return.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.cumulative_open_jump_return",
        &diagnostics.cumulative_open_jump_return.to_string(),
        &expected.cumulative_open_jump_return.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.cumulative_closed_gap_return",
        &diagnostics.cumulative_closed_gap_return.to_string(),
        &expected.cumulative_closed_gap_return.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.cumulative_total_return",
        &diagnostics.cumulative_total_return.to_string(),
        &expected.cumulative_total_return.to_string(),
    )?;
    validate_diagnostics_field(
        "diagnostics.max_abs_total_return",
        &diagnostics.max_abs_total_return.to_string(),
        &expected.max_abs_total_return.to_string(),
    )?;

    Ok(())
}

fn validate_metadata_field(
    field_path: &'static str,
    actual: &str,
    expected: &str,
) -> Result<(), CoarseSimulationPathError> {
    if actual != expected {
        return Err(CoarseSimulationPathError::InvalidOutputSchemaMetadata {
            field_path,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

fn validate_metadata_list_field(
    field_path: &'static str,
    actual: &[String],
    expected: &[String],
) -> Result<(), CoarseSimulationPathError> {
    if actual != expected {
        return Err(CoarseSimulationPathError::InvalidOutputSchemaMetadata {
            field_path,
            expected: format!("{expected:?}"),
            actual: format!("{actual:?}"),
        });
    }
    Ok(())
}

fn validate_provenance_field(
    field_path: &'static str,
    actual: &str,
    expected: &str,
) -> Result<(), CoarseSimulationPathError> {
    if actual != expected {
        return Err(CoarseSimulationPathError::InvalidProvenance {
            field_path,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

fn validate_diagnostics_field(
    field_path: &'static str,
    actual: &str,
    expected: &str,
) -> Result<(), CoarseSimulationPathError> {
    if actual != expected {
        return Err(CoarseSimulationPathError::InvalidDiagnostics {
            field_path,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::affine_oracle::ExponentialAffineTransformEquationSpecification;
    use crate::hawkes::{HawkesKernelLoadings, HawkesKernelParameters};
    use crate::news::GammaSeverityMarkConfiguration;
    use crate::nig::NigSharedPlusShiftConfiguration;

    use super::*;

    #[test]
    fn coarse_run_request_round_trip_serialization_preserves_time_anchor_and_interval_semantics() {
        let request = build_valid_run_request();

        let serialized =
            serde_json::to_value(&request).expect("coarse simulation request should serialize");
        let parsed: CoarseSimulationRunRequest =
            serde_json::from_value(serialized.clone()).expect("serialized request should parse");

        parsed
            .validate()
            .expect("deserialized coarse simulation request should remain valid");
        assert_eq!(parsed, request);
        assert_eq!(
            serialized["schema_version"],
            json!(COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION)
        );
        assert_eq!(
            serialized["start_time_utc_nanos"],
            json!(1_704_067_200_000_000_000_i64)
        );
        assert_eq!(serialized["interval_delta"], json!(1.0));
        assert_eq!(serialized["horizon_interval_count"], json!(2));
        assert_eq!(
            serialized["interval_factors"]["nu_h_known"],
            json!([0.12, 0.03])
        );
    }

    #[test]
    fn coarse_path_round_trip_serialization_preserves_time_indexed_records_and_metadata() {
        let path = build_valid_path();

        let serialized =
            serde_json::to_value(&path).expect("coarse simulation path should serialize");
        let parsed: CoarseSimulationPath =
            serde_json::from_value(serialized.clone()).expect("serialized path should parse");

        parsed
            .validate()
            .expect("deserialized coarse simulation path should remain valid");
        assert_eq!(parsed, path);
        assert_eq!(
            serialized["schema_version"],
            json!(COARSE_SIMULATION_PATH_SCHEMA_VERSION)
        );
        assert_eq!(
            serialized["records"][0]["schema_version"],
            json!(COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION)
        );
        assert_eq!(
            serialized["records"][0]["interval_start_utc_nanos"],
            json!(1_704_067_200_000_000_000_i64)
        );
        assert_eq!(
            serialized["records"][0]["interval_end_utc_nanos"],
            json!(1_704_070_800_000_000_000_i64)
        );
        assert_eq!(
            serialized["output_schema_metadata"]["schema_version"],
            json!(COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION)
        );
        assert_eq!(
            serialized["provenance"]["interval_timing_policy"],
            json!(COARSE_SIMULATION_INTERVAL_TIMING_POLICY)
        );
        assert_eq!(
            serialized["diagnostics"]["last_interval_end_utc_nanos"],
            json!(1_704_074_400_000_000_000_i64)
        );
    }

    #[test]
    fn coarse_path_output_schema_metadata_exposes_required_field_lists() {
        let path = build_valid_path();
        let serialized =
            serde_json::to_value(path).expect("coarse simulation path should serialize");

        assert_eq!(
            serialized["output_schema_metadata"]["required_path_fields"],
            json!([
                "schema_version",
                "run_request_schema_version",
                "output_schema_metadata",
                "provenance",
                "diagnostics",
                "seed",
                "start_time_utc_nanos",
                "end_time_utc_nanos",
                "interval_delta",
                "simulated_interval_count",
                "initial_excitation_state",
                "terminal_excitation_state",
                "records"
            ])
        );
        assert_eq!(
            serialized["output_schema_metadata"]["required_record_fields"],
            json!([
                "schema_version",
                "interval_index",
                "interval_start_utc_nanos",
                "interval_end_utc_nanos",
                "sufficient_statistics",
                "return_components"
            ])
        );
    }

    #[test]
    fn coarse_run_request_rejects_factor_interval_delta_mismatch() {
        let error = CoarseSimulationRunRequest::try_new(
            17,
            1_704_067_200_000_000_000,
            2,
            0.5,
            build_model_inputs(),
            build_interval_factors(Some(1.0)),
        )
        .expect_err("interval delta mismatch must fail");

        assert!(matches!(
            error,
            CoarseSimulationRunRequestError::IntervalDeltaFactorMismatch {
                requested_interval_delta: 0.5,
                available_factor_interval_delta: 1.0,
            }
        ));
    }

    #[test]
    fn coarse_run_request_rejects_non_positive_interval_delta() {
        let error = CoarseSimulationRunRequest::try_new(
            17,
            1_704_067_200_000_000_000,
            2,
            0.0,
            build_model_inputs(),
            build_interval_factors(Some(1.0)),
        )
        .expect_err("non-positive interval delta must fail");

        assert!(matches!(
            error,
            CoarseSimulationRunRequestError::InvalidIntervalDelta {
                source: IntervalDeltaGridError::NonPositiveOrNonFinite { value: 0.0 },
            }
        ));
    }

    #[test]
    fn coarse_run_request_deserialization_rejects_missing_time_anchor_field() {
        let payload = json!({
            "schema_version": COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
            "seed": 17,
            "horizon_interval_count": 2,
            "interval_delta": 1.0,
            "model_inputs": serde_json::to_value(build_model_inputs())
                .expect("model inputs should serialize"),
            "interval_factors": serde_json::to_value(build_interval_factors(Some(1.0)))
                .expect("interval factors should serialize")
        });

        let error = serde_json::from_value::<CoarseSimulationRunRequest>(payload)
            .expect_err("missing time anchor must fail deserialization");

        assert!(error.to_string().contains("start_time_utc_nanos"));
    }

    #[test]
    fn coarse_path_rejects_misaligned_interval_boundary_record() {
        let mut path = build_valid_path();
        path.records[1].interval_start_utc_nanos += 1;

        let error = path
            .validate()
            .expect_err("misaligned interval boundary must fail");

        assert!(matches!(
            error,
            CoarseSimulationPathError::IntervalBoundaryMismatch {
                record_position: 1,
                field_path: "interval_start_utc_nanos",
                ..
            }
        ));
    }

    fn build_valid_run_request() -> CoarseSimulationRunRequest {
        CoarseSimulationRunRequest::try_new(
            17,
            1_704_067_200_000_000_000,
            2,
            1.0,
            build_model_inputs(),
            build_interval_factors(Some(1.0)),
        )
        .expect("valid coarse simulation request should construct")
    }

    fn build_valid_path() -> CoarseSimulationPath {
        let first_record = CoarseSimulationPathRecord::try_new(
            0,
            1_704_067_200_000_000_000,
            1_704_070_800_000_000_000,
            SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 3, 2)
                .expect("statistics should be valid"),
            SufficientStatisticsRiskReturnComponents::try_new(0.04, -0.03, 0.01, 0.0, -0.02, -0.01)
                .expect("return components should be valid"),
        )
        .expect("record should be valid");
        let second_record = CoarseSimulationPathRecord::try_new(
            1,
            1_704_070_800_000_000_000,
            1_704_074_400_000_000_000,
            SufficientStatisticsIntervalStatistics::try_new(0.6, 0.4, 1, 1)
                .expect("statistics should be valid"),
            SufficientStatisticsRiskReturnComponents::try_new(0.01, -0.01, 0.0, 0.01, 0.0, 0.01)
                .expect("return components should be valid"),
        )
        .expect("record should be valid");

        CoarseSimulationPath::try_new(
            17,
            1_704_067_200_000_000_000,
            1.0,
            2,
            build_initial_state(),
            build_initial_state(),
            vec![first_record, second_record],
        )
        .expect("valid coarse simulation path should construct")
    }

    fn build_initial_state() -> HawkesExcitationState {
        HawkesExcitationState {
            y_plus_plus: 0.2,
            y_plus_minus: 0.1,
            y_minus_plus: 0.1,
            y_minus_minus: 0.3,
        }
    }

    fn build_model_inputs() -> SufficientStatisticsRiskModelInputs {
        let hawkes_kernel_parameters = HawkesKernelParameters::try_new(
            0.8,
            1.6,
            HawkesKernelLoadings {
                plus_plus: 0.2,
                plus_minus: 0.1,
                minus_plus: 0.1,
                minus_minus: 0.25,
            },
            HawkesKernelLoadings {
                plus_plus: 0.08,
                plus_minus: 0.04,
                minus_plus: 0.05,
                minus_minus: 0.1,
            },
        )
        .expect("kernel parameters should construct");
        let equation_specification = ExponentialAffineTransformEquationSpecification::try_new(
            hawkes_kernel_parameters,
            GammaSeverityMarkConfiguration::try_new(1.8, 0.7)
                .expect("gamma marks should construct"),
        )
        .expect("equation specification should construct");
        SufficientStatisticsRiskModelInputs::try_new(
            equation_specification,
            NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
                .expect("nig configuration should construct"),
            build_initial_state(),
        )
        .expect("model inputs should construct")
    }

    fn build_interval_factors(
        interval_delta: Option<f64>,
    ) -> SufficientStatisticsRiskIntervalFactors {
        SufficientStatisticsRiskIntervalFactors::try_new(
            vec![1.1, 0.9],
            vec![0.12, 0.03],
            vec![0.08, 0.07],
            interval_delta,
        )
        .expect("interval factors should construct")
    }
}
