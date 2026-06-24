use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::time::utc_datetime_from_nanos;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use serde::{Deserialize, Serialize};

use super::constants::{
    COARSE_SIMULATION_PATH_SCHEMA_VERSION, COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
};
use super::metadata::{
    build_coarse_simulation_path_output_schema_metadata, build_coarse_simulation_path_provenance,
    derive_coarse_simulation_path_diagnostics, derive_path_end_time_utc_nanos,
    validate_diagnostics, validate_output_schema_metadata, validate_provenance,
};
use super::record::{CoarseSimulationPathRecord, CoarseSimulationPathRecordError};
use super::time_index::{
    CoarseSimulationTimeIndexError, ExactStringValidationError, IntervalDeltaGridError,
    compute_horizon_end_time_utc_nanos, interval_delta_to_nanos, validate_exact_string,
    validate_excitation_state,
};

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
    pub cumulative_observed_return: f64,
    pub max_abs_total_return: f64,
    pub max_abs_deviation_from_anchor: f64,
    pub terminal_observed_log_price: f64,
    pub terminal_deviation_from_anchor: f64,
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

        if utc_datetime_from_nanos(self.start_time_utc_nanos).is_none() {
            return Err(CoarseSimulationPathError::InvalidStartTimeUtcNanos {
                value: self.start_time_utc_nanos,
            });
        }

        if utc_datetime_from_nanos(self.end_time_utc_nanos).is_none() {
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

            let expected_interval_start_utc_nanos = compute_horizon_end_time_utc_nanos(
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use janiform_affine_oracle::sufficient_statistics::{
        SufficientStatisticsIntervalStatistics, SufficientStatisticsReturnComponents,
    };
    use janiform_model_kernels::hawkes::HawkesExcitationState;

    use super::{CoarseSimulationPath, CoarseSimulationPathError};
    use crate::coarse::contracts::CoarseSimulationPathRecord;
    use crate::coarse::{
        COARSE_SIMULATION_INTERVAL_TIMING_POLICY,
        COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
        COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION, COARSE_SIMULATION_PATH_SCHEMA_VERSION,
    };

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
                "return_components",
                "core_total_return",
                "observed_interval_return",
                "observed_log_price_end",
                "anchor_value_end",
                "deviation_from_anchor_end"
            ])
        );
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

    fn build_valid_path() -> CoarseSimulationPath {
        let first_record = CoarseSimulationPathRecord::try_new(
            0,
            1_704_067_200_000_000_000,
            1_704_070_800_000_000_000,
            SufficientStatisticsIntervalStatistics::try_new(
                0.7,
                0.0,
                0.5,
                0.0,
                3,
                2,
                build_initial_state(),
                janiform_model_kernels::market_pressure::MarketPressureState::neutral(),
                0.0,
            )
            .expect("statistics should be valid"),
            SufficientStatisticsReturnComponents::try_new(0.04, -0.03, 0.01, 0.0, -0.02, -0.01)
                .expect("return components should be valid"),
            -0.01,
            -0.01,
            -0.01,
            0.0,
            -0.01,
        )
        .expect("record should be valid");

        let second_record = CoarseSimulationPathRecord::try_new(
            1,
            1_704_070_800_000_000_000,
            1_704_074_400_000_000_000,
            SufficientStatisticsIntervalStatistics::try_new(
                0.6,
                0.0,
                0.4,
                0.0,
                1,
                1,
                build_initial_state(),
                janiform_model_kernels::market_pressure::MarketPressureState::neutral(),
                0.0,
            )
            .expect("statistics should be valid"),
            SufficientStatisticsReturnComponents::try_new(0.01, -0.01, 0.0, 0.01, 0.0, 0.01)
                .expect("return components should be valid"),
            0.01,
            0.01,
            0.0,
            0.0,
            0.0,
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
}
