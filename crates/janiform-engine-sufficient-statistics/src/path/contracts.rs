use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionBoundaryStateValidationError,
};

use janiform_affine_oracle::sufficient_statistics::{
    SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
    SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_FACTOR_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY,
    SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY, SufficientStatisticsIntervalStatistics,
    SufficientStatisticsIntervalStatisticsError, SufficientStatisticsReturnComponents,
    SufficientStatisticsReturnComponentsError, validate_excitation_state,
};

use super::super::request::constants::{
    SUFFICIENT_STATISTICS_PATH_ARTIFACT_WORKFLOW_POLICY,
    SUFFICIENT_STATISTICS_PATH_ARTIFACT_WORKFLOW_TAGS,
    SUFFICIENT_STATISTICS_PATH_DIAGNOSTICS_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_PATH_ENVIRONMENT_METADATA_POLICY,
    SUFFICIENT_STATISTICS_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
    SUFFICIENT_STATISTICS_PATH_PROVENANCE_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_PATH_RECORD_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_PATH_REQUIRED_DIAGNOSTICS_FIELDS,
    SUFFICIENT_STATISTICS_PATH_REQUIRED_FIELDS,
    SUFFICIENT_STATISTICS_PATH_REQUIRED_PROVENANCE_FIELDS,
    SUFFICIENT_STATISTICS_PATH_SCHEMA_VERSION, SUFFICIENT_STATISTICS_RUN_REQUEST_SCHEMA_VERSION,
};
/// Machine-readable schema metadata for sufficient-statistics path outputs and downstream artifact validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SufficientStatisticsPathOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub provenance_schema_version: &'static str,
    pub diagnostics_schema_version: &'static str,
    pub artifact_workflow_policy: &'static str,
    pub environment_metadata_policy: &'static str,
    pub required_path_fields: &'static [&'static str],
    pub required_provenance_fields: &'static [&'static str],
    pub required_diagnostics_fields: &'static [&'static str],
}

pub const SUFFICIENT_STATISTICS_PATH_OUTPUT_SCHEMA_METADATA:
    SufficientStatisticsPathOutputSchemaMetadata = SufficientStatisticsPathOutputSchemaMetadata {
    schema_version: SUFFICIENT_STATISTICS_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
    provenance_schema_version: SUFFICIENT_STATISTICS_PATH_PROVENANCE_SCHEMA_VERSION,
    diagnostics_schema_version: SUFFICIENT_STATISTICS_PATH_DIAGNOSTICS_SCHEMA_VERSION,
    artifact_workflow_policy: SUFFICIENT_STATISTICS_PATH_ARTIFACT_WORKFLOW_POLICY,
    environment_metadata_policy: SUFFICIENT_STATISTICS_PATH_ENVIRONMENT_METADATA_POLICY,
    required_path_fields: &SUFFICIENT_STATISTICS_PATH_REQUIRED_FIELDS,
    required_provenance_fields: &SUFFICIENT_STATISTICS_PATH_REQUIRED_PROVENANCE_FIELDS,
    required_diagnostics_fields: &SUFFICIENT_STATISTICS_PATH_REQUIRED_DIAGNOSTICS_FIELDS,
};

/// Deterministic provenance payload for reproducible sufficient-statistics path artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SufficientStatisticsPathProvenance {
    pub schema_version: &'static str,
    pub artifact_workflow_policy: &'static str,
    pub environment_metadata_policy: &'static str,
    pub interval_factor_policy: &'static str,
    pub transform_slice_sampling_policy: &'static str,
    pub boundary_state_update_policy: &'static str,
    pub interval_return_generation_policy: &'static str,
    pub interval_return_component_routing_policy: &'static str,
    pub batch_seed_partition_policy: &'static str,
    pub artifact_workflow_tags: &'static [&'static str],
}

pub const SUFFICIENT_STATISTICS_PATH_PROVENANCE: SufficientStatisticsPathProvenance =
    SufficientStatisticsPathProvenance {
        schema_version: SUFFICIENT_STATISTICS_PATH_PROVENANCE_SCHEMA_VERSION,
        artifact_workflow_policy: SUFFICIENT_STATISTICS_PATH_ARTIFACT_WORKFLOW_POLICY,
        environment_metadata_policy: SUFFICIENT_STATISTICS_PATH_ENVIRONMENT_METADATA_POLICY,
        interval_factor_policy: SUFFICIENT_STATISTICS_INTERVAL_FACTOR_POLICY,
        transform_slice_sampling_policy: SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY,
        boundary_state_update_policy: SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
        interval_return_generation_policy: SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY,
        interval_return_component_routing_policy:
            SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
        batch_seed_partition_policy: SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
        artifact_workflow_tags: &SUFFICIENT_STATISTICS_PATH_ARTIFACT_WORKFLOW_TAGS,
    };

/// Concise deterministic diagnostics for triage and downstream validation reports.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsPathDiagnostics {
    pub schema_version: &'static str,
    pub simulated_interval_count: usize,
    pub first_interval_index: Option<usize>,
    pub last_interval_index: Option<usize>,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsPathRecord {
    pub schema_version: &'static str,
    pub interval_index: usize,
    pub sufficient_statistics: SufficientStatisticsIntervalStatistics,
    pub return_components: SufficientStatisticsReturnComponents,
    pub core_total_return: f64,
    pub observed_interval_return: f64,
    pub observed_log_price_end: f64,
    pub anchor_value_end: f64,
    pub deviation_from_anchor_end: f64,
}

impl SufficientStatisticsPathRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        interval_index: usize,
        sufficient_statistics: SufficientStatisticsIntervalStatistics,
        return_components: SufficientStatisticsReturnComponents,
        core_total_return: f64,
        observed_interval_return: f64,
        observed_log_price_end: f64,
        anchor_value_end: f64,
        deviation_from_anchor_end: f64,
    ) -> Result<Self, SufficientStatisticsPathRecordError> {
        let path_record = Self {
            schema_version: SUFFICIENT_STATISTICS_PATH_RECORD_SCHEMA_VERSION,
            interval_index,
            sufficient_statistics,
            return_components,
            core_total_return,
            observed_interval_return,
            observed_log_price_end,
            anchor_value_end,
            deviation_from_anchor_end,
        };

        path_record.validate()?;

        Ok(path_record)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsPathRecordError> {
        self.sufficient_statistics.validate().map_err(|source| {
            SufficientStatisticsPathRecordError::InvalidSufficientStatistics { source }
        })?;

        self.return_components.validate().map_err(|source| {
            SufficientStatisticsPathRecordError::InvalidReturnComponents { source }
        })?;

        if self.core_total_return != self.return_components.total_return {
            return Err(
                SufficientStatisticsPathRecordError::CoreTotalReturnMismatch {
                    expected_core_total_return: self.return_components.total_return,
                    observed_core_total_return: self.core_total_return,
                },
            );
        }

        if !self.observed_interval_return.is_finite() {
            return Err(
                SufficientStatisticsPathRecordError::InvalidObservedIntervalReturn {
                    value: self.observed_interval_return,
                },
            );
        }

        MeanReversionBoundaryState::try_new(
            self.observed_log_price_end,
            self.anchor_value_end,
            self.deviation_from_anchor_end,
        )
        .map_err(|source| {
            SufficientStatisticsPathRecordError::InvalidObservedBoundaryState { source }
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsPathRecordError {
    InvalidSufficientStatistics {
        source: SufficientStatisticsIntervalStatisticsError,
    },
    InvalidReturnComponents {
        source: SufficientStatisticsReturnComponentsError,
    },
    CoreTotalReturnMismatch {
        expected_core_total_return: f64,
        observed_core_total_return: f64,
    },
    InvalidObservedIntervalReturn {
        value: f64,
    },
    InvalidObservedBoundaryState {
        source: MeanReversionBoundaryStateValidationError,
    },
}

impl Display for SufficientStatisticsPathRecordError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSufficientStatistics { source } => {
                write!(formatter, "sufficient_statistics is invalid: {source}")
            }
            Self::InvalidReturnComponents { source } => {
                write!(formatter, "return_components is invalid: {source}")
            }
            Self::CoreTotalReturnMismatch {
                expected_core_total_return,
                observed_core_total_return,
            } => write!(
                formatter,
                "core_total_return must equal return_components.total_return ({expected_core_total_return}), found {observed_core_total_return}"
            ),
            Self::InvalidObservedIntervalReturn { value } => write!(
                formatter,
                "observed_interval_return must be finite, found {value}"
            ),
            Self::InvalidObservedBoundaryState { source } => {
                write!(
                    formatter,
                    "observed end-of-interval boundary state is invalid: {source}"
                )
            }
        }
    }
}

impl Error for SufficientStatisticsPathRecordError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSufficientStatistics { source } => Some(source),
            Self::InvalidReturnComponents { source } => Some(source),
            Self::InvalidObservedBoundaryState { source } => Some(source),
            Self::CoreTotalReturnMismatch { .. } | Self::InvalidObservedIntervalReturn { .. } => {
                None
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsPath {
    pub schema_version: &'static str,
    pub run_request_schema_version: &'static str,
    pub output_schema_metadata: SufficientStatisticsPathOutputSchemaMetadata,
    pub provenance: SufficientStatisticsPathProvenance,
    pub diagnostics: SufficientStatisticsPathDiagnostics,
    pub seed: u64,
    pub interval_delta: f64,
    pub simulated_interval_count: usize,
    pub initial_excitation_state: HawkesExcitationState,
    pub terminal_excitation_state: HawkesExcitationState,
    pub records: Vec<SufficientStatisticsPathRecord>,
}

impl SufficientStatisticsPath {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        seed: u64,
        interval_delta: f64,
        simulated_interval_count: usize,
        initial_excitation_state: HawkesExcitationState,
        terminal_excitation_state: HawkesExcitationState,
        records: Vec<SufficientStatisticsPathRecord>,
    ) -> Result<Self, SufficientStatisticsPathError> {
        let diagnostics =
            derive_sufficient_statistics_path_diagnostics(simulated_interval_count, &records)?;

        let path = Self {
            schema_version: SUFFICIENT_STATISTICS_PATH_SCHEMA_VERSION,
            run_request_schema_version: SUFFICIENT_STATISTICS_RUN_REQUEST_SCHEMA_VERSION,
            output_schema_metadata: SUFFICIENT_STATISTICS_PATH_OUTPUT_SCHEMA_METADATA,
            provenance: SUFFICIENT_STATISTICS_PATH_PROVENANCE,
            diagnostics,
            seed,
            interval_delta,
            simulated_interval_count,
            initial_excitation_state,
            terminal_excitation_state,
            records,
        };

        path.validate()?;

        Ok(path)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsPathError> {
        if !self.interval_delta.is_finite() || self.interval_delta <= 0.0 {
            return Err(SufficientStatisticsPathError::InvalidIntervalDelta {
                value: self.interval_delta,
            });
        }

        if self.simulated_interval_count == 0 {
            return Err(SufficientStatisticsPathError::InvalidSimulatedIntervalCount { value: 0 });
        }

        validate_sufficient_statistics_path_output_schema_metadata(self.output_schema_metadata)?;
        validate_sufficient_statistics_path_provenance(self.provenance)?;

        if self.records.len() != self.simulated_interval_count {
            return Err(SufficientStatisticsPathError::RecordCountMismatch {
                simulated_interval_count: self.simulated_interval_count,
                record_count: self.records.len(),
            });
        }

        validate_excitation_state(self.initial_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsPathError::InvalidInitialState { field, value }
        })?;

        validate_excitation_state(self.terminal_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsPathError::InvalidTerminalState { field, value }
        })?;

        for (record_position, record) in self.records.iter().enumerate() {
            if record.interval_index != record_position {
                return Err(SufficientStatisticsPathError::NonSequentialIntervalIndex {
                    record_position,
                    record_interval_index: record.interval_index,
                });
            }

            record
                .validate()
                .map_err(|source| SufficientStatisticsPathError::InvalidRecord {
                    record_position,
                    source,
                })?;
        }

        validate_sufficient_statistics_path_diagnostics(
            self.simulated_interval_count,
            &self.records,
            self.diagnostics,
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsPathError {
    InvalidIntervalDelta {
        value: f64,
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
    NonSequentialIntervalIndex {
        record_position: usize,
        record_interval_index: usize,
    },
    InvalidRecord {
        record_position: usize,
        source: SufficientStatisticsPathRecordError,
    },
    InvalidOutputSchemaMetadataField {
        field_path: &'static str,
        expected: String,
        actual: String,
    },
    InvalidProvenanceField {
        field_path: &'static str,
        expected: String,
        actual: String,
    },
    InvalidDiagnosticsField {
        field_path: String,
        expected: String,
        actual: String,
    },
}

impl Display for SufficientStatisticsPathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_delta must be finite and strictly positive, found {value}"
            ),
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
            Self::NonSequentialIntervalIndex {
                record_position,
                record_interval_index,
            } => write!(
                formatter,
                "records[{record_position}].interval_index must equal {record_position}, found {record_interval_index}"
            ),
            Self::InvalidRecord {
                record_position,
                source,
            } => write!(formatter, "records[{record_position}] is invalid: {source}"),
            Self::InvalidOutputSchemaMetadataField {
                field_path,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_path} must be {expected} for deterministic sufficient-statistics path schema metadata, found {actual}"
            ),
            Self::InvalidProvenanceField {
                field_path,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_path} must be {expected} for deterministic sufficient-statistics path provenance, found {actual}"
            ),
            Self::InvalidDiagnosticsField {
                field_path,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_path} must be {expected} for deterministic sufficient-statistics path diagnostics, found {actual}"
            ),
        }
    }
}

impl Error for SufficientStatisticsPathError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRecord { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn validate_sufficient_statistics_path_output_schema_metadata(
    metadata: SufficientStatisticsPathOutputSchemaMetadata,
) -> Result<(), SufficientStatisticsPathError> {
    let expected = SUFFICIENT_STATISTICS_PATH_OUTPUT_SCHEMA_METADATA;

    if metadata.schema_version != expected.schema_version {
        return Err(
            SufficientStatisticsPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.schema_version",
                expected: expected.schema_version.to_string(),
                actual: metadata.schema_version.to_string(),
            },
        );
    }

    if metadata.provenance_schema_version != expected.provenance_schema_version {
        return Err(
            SufficientStatisticsPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.provenance_schema_version",
                expected: expected.provenance_schema_version.to_string(),
                actual: metadata.provenance_schema_version.to_string(),
            },
        );
    }

    if metadata.diagnostics_schema_version != expected.diagnostics_schema_version {
        return Err(
            SufficientStatisticsPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.diagnostics_schema_version",
                expected: expected.diagnostics_schema_version.to_string(),
                actual: metadata.diagnostics_schema_version.to_string(),
            },
        );
    }

    if metadata.artifact_workflow_policy != expected.artifact_workflow_policy {
        return Err(
            SufficientStatisticsPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.artifact_workflow_policy",
                expected: expected.artifact_workflow_policy.to_string(),
                actual: metadata.artifact_workflow_policy.to_string(),
            },
        );
    }

    if metadata.environment_metadata_policy != expected.environment_metadata_policy {
        return Err(
            SufficientStatisticsPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.environment_metadata_policy",
                expected: expected.environment_metadata_policy.to_string(),
                actual: metadata.environment_metadata_policy.to_string(),
            },
        );
    }

    if metadata.required_path_fields != expected.required_path_fields {
        return Err(
            SufficientStatisticsPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.required_path_fields",
                expected: format!("{:?}", expected.required_path_fields),
                actual: format!("{:?}", metadata.required_path_fields),
            },
        );
    }

    if metadata.required_provenance_fields != expected.required_provenance_fields {
        return Err(
            SufficientStatisticsPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.required_provenance_fields",
                expected: format!("{:?}", expected.required_provenance_fields),
                actual: format!("{:?}", metadata.required_provenance_fields),
            },
        );
    }

    if metadata.required_diagnostics_fields != expected.required_diagnostics_fields {
        return Err(
            SufficientStatisticsPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.required_diagnostics_fields",
                expected: format!("{:?}", expected.required_diagnostics_fields),
                actual: format!("{:?}", metadata.required_diagnostics_fields),
            },
        );
    }

    Ok(())
}

fn validate_sufficient_statistics_path_provenance(
    provenance: SufficientStatisticsPathProvenance,
) -> Result<(), SufficientStatisticsPathError> {
    let expected = SUFFICIENT_STATISTICS_PATH_PROVENANCE;

    if provenance.schema_version != expected.schema_version {
        return Err(SufficientStatisticsPathError::InvalidProvenanceField {
            field_path: "provenance.schema_version",
            expected: expected.schema_version.to_string(),
            actual: provenance.schema_version.to_string(),
        });
    }

    if provenance.artifact_workflow_policy != expected.artifact_workflow_policy {
        return Err(SufficientStatisticsPathError::InvalidProvenanceField {
            field_path: "provenance.artifact_workflow_policy",
            expected: expected.artifact_workflow_policy.to_string(),
            actual: provenance.artifact_workflow_policy.to_string(),
        });
    }

    if provenance.environment_metadata_policy != expected.environment_metadata_policy {
        return Err(SufficientStatisticsPathError::InvalidProvenanceField {
            field_path: "provenance.environment_metadata_policy",
            expected: expected.environment_metadata_policy.to_string(),
            actual: provenance.environment_metadata_policy.to_string(),
        });
    }

    if provenance.interval_factor_policy != expected.interval_factor_policy {
        return Err(SufficientStatisticsPathError::InvalidProvenanceField {
            field_path: "provenance.interval_factor_policy",
            expected: expected.interval_factor_policy.to_string(),
            actual: provenance.interval_factor_policy.to_string(),
        });
    }

    if provenance.transform_slice_sampling_policy != expected.transform_slice_sampling_policy {
        return Err(SufficientStatisticsPathError::InvalidProvenanceField {
            field_path: "provenance.transform_slice_sampling_policy",
            expected: expected.transform_slice_sampling_policy.to_string(),
            actual: provenance.transform_slice_sampling_policy.to_string(),
        });
    }

    if provenance.boundary_state_update_policy != expected.boundary_state_update_policy {
        return Err(SufficientStatisticsPathError::InvalidProvenanceField {
            field_path: "provenance.boundary_state_update_policy",
            expected: expected.boundary_state_update_policy.to_string(),
            actual: provenance.boundary_state_update_policy.to_string(),
        });
    }

    if provenance.interval_return_generation_policy != expected.interval_return_generation_policy {
        return Err(SufficientStatisticsPathError::InvalidProvenanceField {
            field_path: "provenance.interval_return_generation_policy",
            expected: expected.interval_return_generation_policy.to_string(),
            actual: provenance.interval_return_generation_policy.to_string(),
        });
    }

    if provenance.interval_return_component_routing_policy
        != expected.interval_return_component_routing_policy
    {
        return Err(SufficientStatisticsPathError::InvalidProvenanceField {
            field_path: "provenance.interval_return_component_routing_policy",
            expected: expected
                .interval_return_component_routing_policy
                .to_string(),
            actual: provenance
                .interval_return_component_routing_policy
                .to_string(),
        });
    }

    if provenance.batch_seed_partition_policy != expected.batch_seed_partition_policy {
        return Err(SufficientStatisticsPathError::InvalidProvenanceField {
            field_path: "provenance.batch_seed_partition_policy",
            expected: expected.batch_seed_partition_policy.to_string(),
            actual: provenance.batch_seed_partition_policy.to_string(),
        });
    }

    if provenance.artifact_workflow_tags != expected.artifact_workflow_tags {
        return Err(SufficientStatisticsPathError::InvalidProvenanceField {
            field_path: "provenance.artifact_workflow_tags",
            expected: format!("{:?}", expected.artifact_workflow_tags),
            actual: format!("{:?}", provenance.artifact_workflow_tags),
        });
    }

    Ok(())
}

fn derive_sufficient_statistics_path_diagnostics(
    simulated_interval_count: usize,
    records: &[SufficientStatisticsPathRecord],
) -> Result<SufficientStatisticsPathDiagnostics, SufficientStatisticsPathError> {
    let mut cumulative_a_plus = 0.0;
    let mut cumulative_a_minus = 0.0;
    let mut cumulative_n_plus: u64 = 0;
    let mut cumulative_n_minus: u64 = 0;

    let mut cumulative_trading_time_return = 0.0;
    let mut cumulative_open_jump_return = 0.0;
    let mut cumulative_closed_gap_return = 0.0;
    let mut cumulative_total_return = 0.0;
    let mut cumulative_observed_return = 0.0;

    let mut max_abs_total_return: f64 = 0.0;
    let mut max_abs_deviation_from_anchor: f64 = 0.0;

    for (record_position, record) in records.iter().enumerate() {
        cumulative_a_plus += record.sufficient_statistics.a_plus;
        if !cumulative_a_plus.is_finite() {
            return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_a_plus".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].sufficient_statistics.a_plus aggregation"
                ),
            });
        }

        cumulative_a_minus += record.sufficient_statistics.a_minus;
        if !cumulative_a_minus.is_finite() {
            return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_a_minus".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].sufficient_statistics.a_minus aggregation"
                ),
            });
        }

        cumulative_n_plus = cumulative_n_plus
            .checked_add(record.sufficient_statistics.n_plus)
            .ok_or(SufficientStatisticsPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_n_plus".to_string(),
                expected: "non-overflowing u64 cumulative count".to_string(),
                actual: "overflow during cumulative_n_plus aggregation".to_string(),
            })?;

        cumulative_n_minus = cumulative_n_minus
            .checked_add(record.sufficient_statistics.n_minus)
            .ok_or(SufficientStatisticsPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_n_minus".to_string(),
                expected: "non-overflowing u64 cumulative count".to_string(),
                actual: "overflow during cumulative_n_minus aggregation".to_string(),
            })?;

        cumulative_trading_time_return += record.return_components.trading_time_return;
        if !cumulative_trading_time_return.is_finite() {
            return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_trading_time_return".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].return_components.trading_time_return aggregation"
                ),
            });
        }

        cumulative_open_jump_return += record.return_components.open_jump_return;
        if !cumulative_open_jump_return.is_finite() {
            return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_open_jump_return".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].return_components.open_jump_return aggregation"
                ),
            });
        }

        cumulative_closed_gap_return += record.return_components.closed_gap_return;
        if !cumulative_closed_gap_return.is_finite() {
            return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_closed_gap_return".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].return_components.closed_gap_return aggregation"
                ),
            });
        }

        cumulative_total_return += record.return_components.total_return;
        if !cumulative_total_return.is_finite() {
            return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_total_return".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].return_components.total_return aggregation"
                ),
            });
        }

        let absolute_total_return = record.return_components.total_return.abs();
        if !absolute_total_return.is_finite() {
            return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.max_abs_total_return".to_string(),
                expected: "finite absolute total return".to_string(),
                actual: format!(
                    "non-finite value at records[{record_position}].return_components.total_return"
                ),
            });
        }

        max_abs_total_return = max_abs_total_return.max(absolute_total_return);

        cumulative_observed_return += record.observed_interval_return;
        if !cumulative_observed_return.is_finite() {
            return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_observed_return".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].observed_interval_return aggregation"
                ),
            });
        }

        let absolute_deviation_from_anchor = record.deviation_from_anchor_end.abs();
        if !absolute_deviation_from_anchor.is_finite() {
            return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.max_abs_deviation_from_anchor".to_string(),
                expected: "finite absolute deviation".to_string(),
                actual: format!(
                    "non-finite value at records[{record_position}].deviation_from_anchor_end"
                ),
            });
        }

        max_abs_deviation_from_anchor =
            max_abs_deviation_from_anchor.max(absolute_deviation_from_anchor);
    }

    let terminal_observed_log_price = records
        .last()
        .map(|record| record.observed_log_price_end)
        .unwrap_or(0.0);

    let terminal_deviation_from_anchor = records
        .last()
        .map(|record| record.deviation_from_anchor_end)
        .unwrap_or(0.0);

    Ok(SufficientStatisticsPathDiagnostics {
        schema_version: SUFFICIENT_STATISTICS_PATH_DIAGNOSTICS_SCHEMA_VERSION,
        simulated_interval_count,
        first_interval_index: records.first().map(|record| record.interval_index),
        last_interval_index: records.last().map(|record| record.interval_index),
        cumulative_a_plus,
        cumulative_a_minus,
        cumulative_n_plus,
        cumulative_n_minus,
        cumulative_trading_time_return,
        cumulative_open_jump_return,
        cumulative_closed_gap_return,
        cumulative_total_return,
        cumulative_observed_return,
        max_abs_total_return,
        max_abs_deviation_from_anchor,
        terminal_observed_log_price,
        terminal_deviation_from_anchor,
    })
}

fn validate_sufficient_statistics_path_diagnostics(
    simulated_interval_count: usize,
    records: &[SufficientStatisticsPathRecord],
    diagnostics: SufficientStatisticsPathDiagnostics,
) -> Result<(), SufficientStatisticsPathError> {
    let expected_diagnostics =
        derive_sufficient_statistics_path_diagnostics(simulated_interval_count, records)?;

    if diagnostics.schema_version != expected_diagnostics.schema_version {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.schema_version".to_string(),
            expected: expected_diagnostics.schema_version.to_string(),
            actual: diagnostics.schema_version.to_string(),
        });
    }

    if diagnostics.simulated_interval_count != expected_diagnostics.simulated_interval_count {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.simulated_interval_count".to_string(),
            expected: expected_diagnostics.simulated_interval_count.to_string(),
            actual: diagnostics.simulated_interval_count.to_string(),
        });
    }

    if diagnostics.first_interval_index != expected_diagnostics.first_interval_index {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.first_interval_index".to_string(),
            expected: format!("{:?}", expected_diagnostics.first_interval_index),
            actual: format!("{:?}", diagnostics.first_interval_index),
        });
    }

    if diagnostics.last_interval_index != expected_diagnostics.last_interval_index {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.last_interval_index".to_string(),
            expected: format!("{:?}", expected_diagnostics.last_interval_index),
            actual: format!("{:?}", diagnostics.last_interval_index),
        });
    }

    if diagnostics.cumulative_a_plus != expected_diagnostics.cumulative_a_plus {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_a_plus".to_string(),
            expected: expected_diagnostics.cumulative_a_plus.to_string(),
            actual: diagnostics.cumulative_a_plus.to_string(),
        });
    }

    if diagnostics.cumulative_a_minus != expected_diagnostics.cumulative_a_minus {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_a_minus".to_string(),
            expected: expected_diagnostics.cumulative_a_minus.to_string(),
            actual: diagnostics.cumulative_a_minus.to_string(),
        });
    }

    if diagnostics.cumulative_n_plus != expected_diagnostics.cumulative_n_plus {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_n_plus".to_string(),
            expected: expected_diagnostics.cumulative_n_plus.to_string(),
            actual: diagnostics.cumulative_n_plus.to_string(),
        });
    }

    if diagnostics.cumulative_n_minus != expected_diagnostics.cumulative_n_minus {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_n_minus".to_string(),
            expected: expected_diagnostics.cumulative_n_minus.to_string(),
            actual: diagnostics.cumulative_n_minus.to_string(),
        });
    }

    if diagnostics.cumulative_trading_time_return
        != expected_diagnostics.cumulative_trading_time_return
    {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_trading_time_return".to_string(),
            expected: expected_diagnostics
                .cumulative_trading_time_return
                .to_string(),
            actual: diagnostics.cumulative_trading_time_return.to_string(),
        });
    }

    if diagnostics.cumulative_open_jump_return != expected_diagnostics.cumulative_open_jump_return {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_open_jump_return".to_string(),
            expected: expected_diagnostics.cumulative_open_jump_return.to_string(),
            actual: diagnostics.cumulative_open_jump_return.to_string(),
        });
    }

    if diagnostics.cumulative_closed_gap_return != expected_diagnostics.cumulative_closed_gap_return
    {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_closed_gap_return".to_string(),
            expected: expected_diagnostics
                .cumulative_closed_gap_return
                .to_string(),
            actual: diagnostics.cumulative_closed_gap_return.to_string(),
        });
    }

    if diagnostics.cumulative_total_return != expected_diagnostics.cumulative_total_return {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_total_return".to_string(),
            expected: expected_diagnostics.cumulative_total_return.to_string(),
            actual: diagnostics.cumulative_total_return.to_string(),
        });
    }

    if diagnostics.cumulative_observed_return != expected_diagnostics.cumulative_observed_return {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_observed_return".to_string(),
            expected: expected_diagnostics.cumulative_observed_return.to_string(),
            actual: diagnostics.cumulative_observed_return.to_string(),
        });
    }

    if diagnostics.max_abs_total_return != expected_diagnostics.max_abs_total_return {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.max_abs_total_return".to_string(),
            expected: expected_diagnostics.max_abs_total_return.to_string(),
            actual: diagnostics.max_abs_total_return.to_string(),
        });
    }

    if diagnostics.max_abs_deviation_from_anchor
        != expected_diagnostics.max_abs_deviation_from_anchor
    {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.max_abs_deviation_from_anchor".to_string(),
            expected: expected_diagnostics
                .max_abs_deviation_from_anchor
                .to_string(),
            actual: diagnostics.max_abs_deviation_from_anchor.to_string(),
        });
    }

    if diagnostics.terminal_observed_log_price != expected_diagnostics.terminal_observed_log_price {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.terminal_observed_log_price".to_string(),
            expected: expected_diagnostics.terminal_observed_log_price.to_string(),
            actual: diagnostics.terminal_observed_log_price.to_string(),
        });
    }

    if diagnostics.terminal_deviation_from_anchor
        != expected_diagnostics.terminal_deviation_from_anchor
    {
        return Err(SufficientStatisticsPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.terminal_deviation_from_anchor".to_string(),
            expected: expected_diagnostics
                .terminal_deviation_from_anchor
                .to_string(),
            actual: diagnostics.terminal_deviation_from_anchor.to_string(),
        });
    }

    Ok(())
}
