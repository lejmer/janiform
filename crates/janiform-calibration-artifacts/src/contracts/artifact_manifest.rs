use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::strings::is_non_empty_string;
use janiform_foundations::time::parse_utc_datetime;
use serde::{Deserialize, Serialize};

use super::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY, CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION,
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CalibrationExecutionMode, CalibrationStage,
};

/// Minimal top-level manifest for persisted calibration artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationArtifactManifest {
    pub schema_version: String,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub index_metadata: CalibrationArtifactManifestIndexMetadata,
    pub artifact_references: Vec<CalibrationArtifactReference>,
}

impl CalibrationArtifactManifest {
    pub fn try_new(
        run_id: impl Into<String>,
        index_metadata: CalibrationArtifactManifestIndexMetadata,
        artifact_references: Vec<CalibrationArtifactReference>,
    ) -> Result<Self, CalibrationArtifactManifestError> {
        let manifest = Self {
            schema_version: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
            run_id: run_id.into(),
            execution_mode: CalibrationExecutionMode::Disk,
            index_metadata,
            artifact_references,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), CalibrationArtifactManifestError> {
        validate_artifact_manifest_schema_version(self.schema_version.as_str())?;

        if self.run_id.trim().is_empty() {
            return Err(CalibrationArtifactManifestError::MissingRunId);
        }

        if self.execution_mode != CalibrationExecutionMode::Disk {
            return Err(CalibrationArtifactManifestError::UnsupportedExecutionMode {
                expected: CalibrationExecutionMode::Disk,
                found: self.execution_mode,
            });
        }
        self.index_metadata.validate()?;

        if self.artifact_references.is_empty() {
            return Err(CalibrationArtifactManifestError::MissingArtifactReferences);
        }

        let mut artifact_ids = BTreeSet::new();
        for artifact_reference in &self.artifact_references {
            if artifact_reference.artifact_id.trim().is_empty() {
                return Err(CalibrationArtifactManifestError::EmptyArtifactId);
            }

            if artifact_reference.artifact_reference.trim().is_empty() {
                return Err(CalibrationArtifactManifestError::EmptyArtifactReference {
                    artifact_id: artifact_reference.artifact_id.clone(),
                });
            }

            if !artifact_ids.insert(artifact_reference.artifact_id.clone()) {
                return Err(CalibrationArtifactManifestError::DuplicateArtifactId {
                    artifact_id: artifact_reference.artifact_id.clone(),
                });
            }
        }

        Ok(())
    }

    pub const fn schema_version() -> &'static str {
        CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationArtifactManifestIndexMetadata {
    pub committed_at_utc: String,
    pub committed_at_unix_nanos: i64,
    pub exchange_code: String,
    pub dataset_snapshot_checksum: String,
    pub trade_events_data_reference: String,
    pub trade_events_reference_checksum: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_surface_data_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_surface_reference_checksum: Option<String>,
    pub model_version: CalibrationArtifactManifestModelVersion,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validation_metric_summaries: Vec<CalibrationArtifactManifestValidationMetricSummary>,
}

impl CalibrationArtifactManifestIndexMetadata {
    pub fn validate(&self) -> Result<(), CalibrationArtifactManifestError> {
        if self.committed_at_utc.trim().is_empty() {
            return Err(
                CalibrationArtifactManifestError::MissingIndexMetadataField {
                    field: "index_metadata.committed_at_utc",
                },
            );
        }
        if self.exchange_code.trim().is_empty() {
            return Err(
                CalibrationArtifactManifestError::MissingIndexMetadataField {
                    field: "index_metadata.exchange_code",
                },
            );
        }
        if self.dataset_snapshot_checksum.trim().is_empty() {
            return Err(
                CalibrationArtifactManifestError::MissingIndexMetadataField {
                    field: "index_metadata.dataset_snapshot_checksum",
                },
            );
        }
        if self.trade_events_data_reference.trim().is_empty() {
            return Err(
                CalibrationArtifactManifestError::MissingIndexMetadataField {
                    field: "index_metadata.trade_events_data_reference",
                },
            );
        }
        if self.trade_events_reference_checksum.trim().is_empty() {
            return Err(
                CalibrationArtifactManifestError::MissingIndexMetadataField {
                    field: "index_metadata.trade_events_reference_checksum",
                },
            );
        }

        validate_checksum_hex(
            "index_metadata.dataset_snapshot_checksum",
            self.dataset_snapshot_checksum.as_str(),
        )?;
        validate_reference_checksum(
            "index_metadata.trade_events_reference_checksum",
            self.trade_events_data_reference.as_str(),
            self.trade_events_reference_checksum.as_str(),
        )?;

        match (
            self.option_surface_data_reference.as_ref(),
            self.option_surface_reference_checksum.as_ref(),
        ) {
            (Some(data_reference), Some(reference_checksum)) => {
                if data_reference.trim().is_empty() {
                    return Err(
                        CalibrationArtifactManifestError::MissingIndexMetadataField {
                            field: "index_metadata.option_surface_data_reference",
                        },
                    );
                }
                if reference_checksum.trim().is_empty() {
                    return Err(
                        CalibrationArtifactManifestError::MissingIndexMetadataField {
                            field: "index_metadata.option_surface_reference_checksum",
                        },
                    );
                }
                validate_reference_checksum(
                    "index_metadata.option_surface_reference_checksum",
                    data_reference.as_str(),
                    reference_checksum.as_str(),
                )?;
            }
            (None, None) => {}
            _ => {
                return Err(
                    CalibrationArtifactManifestError::OptionalIndexMetadataPairMismatch {
                        first_field: "index_metadata.option_surface_data_reference",
                        second_field: "index_metadata.option_surface_reference_checksum",
                    },
                );
            }
        }

        let committed_at = parse_utc_datetime(self.committed_at_utc.as_str()).map_err(|_| {
            CalibrationArtifactManifestError::InvalidCommittedAtTimestamp {
                found: self.committed_at_utc.clone(),
            }
        })?;

        let expected_unix_nanos = committed_at.timestamp_nanos_opt().ok_or(
            CalibrationArtifactManifestError::InvalidCommittedAtTimestamp {
                found: self.committed_at_utc.clone(),
            },
        )?;
        if expected_unix_nanos != self.committed_at_unix_nanos {
            return Err(
                CalibrationArtifactManifestError::CommittedAtTimestampMismatch {
                    committed_at_utc: self.committed_at_utc.clone(),
                    committed_at_unix_nanos: self.committed_at_unix_nanos,
                    expected_unix_nanos,
                },
            );
        }

        self.model_version.validate()?;

        let mut validation_summary_roles = BTreeSet::new();
        for validation_summary in &self.validation_metric_summaries {
            if !validation_summary_roles.insert(validation_summary.artifact_role) {
                return Err(
                    CalibrationArtifactManifestError::DuplicateValidationMetricSummaryRole {
                        artifact_role: validation_summary.artifact_role,
                    },
                );
            }
            validation_summary.validate()?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationArtifactManifestModelVersion {
    pub run_contract_schema_version: String,
    pub run_spec_schema_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seasonality_stage_output_schema_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hawkes_stage_output_schema_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_components_stage_output_schema_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_reversion_stage_output_schema_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joint_refinement_stage_output_schema_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seasonality_estimator_version: Option<String>,
}

impl CalibrationArtifactManifestModelVersion {
    fn validate(&self) -> Result<(), CalibrationArtifactManifestError> {
        for (field, value) in [
            (
                "index_metadata.model_version.run_contract_schema_version",
                self.run_contract_schema_version.as_str(),
            ),
            (
                "index_metadata.model_version.run_spec_schema_version",
                self.run_spec_schema_version.as_str(),
            ),
        ] {
            if !is_non_empty_string(value) {
                return Err(CalibrationArtifactManifestError::MissingIndexMetadataField { field });
            }
        }

        for (field, value) in [
            (
                "index_metadata.model_version.seasonality_stage_output_schema_version",
                self.seasonality_stage_output_schema_version.as_ref(),
            ),
            (
                "index_metadata.model_version.hawkes_stage_output_schema_version",
                self.hawkes_stage_output_schema_version.as_ref(),
            ),
            (
                "index_metadata.model_version.return_components_stage_output_schema_version",
                self.return_components_stage_output_schema_version.as_ref(),
            ),
            (
                "index_metadata.model_version.mean_reversion_stage_output_schema_version",
                self.mean_reversion_stage_output_schema_version.as_ref(),
            ),
            (
                "index_metadata.model_version.joint_refinement_stage_output_schema_version",
                self.joint_refinement_stage_output_schema_version.as_ref(),
            ),
            (
                "index_metadata.model_version.seasonality_estimator_version",
                self.seasonality_estimator_version.as_ref(),
            ),
        ] {
            if value.is_some_and(|value| !is_non_empty_string(value)) {
                return Err(CalibrationArtifactManifestError::MissingIndexMetadataField { field });
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationArtifactManifestValidationMetricRole {
    Diagnostics,
    Comparison,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationArtifactManifestMetricStatus {
    Pass,
    Fail,
    InsufficientData,
    Available,
    Missing,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationArtifactManifestMetricStatusCount {
    pub status: CalibrationArtifactManifestMetricStatus,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationArtifactManifestValidationMetricSummary {
    pub artifact_role: CalibrationArtifactManifestValidationMetricRole,
    pub artifact_reference: String,
    pub overall_status: CalibrationArtifactManifestMetricStatus,
    pub collection_count: usize,
    pub metric_count: usize,
    pub collection_status_counts: Vec<CalibrationArtifactManifestMetricStatusCount>,
    pub metric_status_counts: Vec<CalibrationArtifactManifestMetricStatusCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failing_metric_ids: Vec<String>,
}

impl CalibrationArtifactManifestValidationMetricSummary {
    fn validate(&self) -> Result<(), CalibrationArtifactManifestError> {
        if self.artifact_reference.trim().is_empty() {
            return Err(
                CalibrationArtifactManifestError::MissingIndexMetadataField {
                    field: "index_metadata.validation_metric_summaries[].artifact_reference",
                },
            );
        }
        if self.collection_count == 0 {
            return Err(
                CalibrationArtifactManifestError::InvalidValidationMetricSummary {
                    artifact_role: self.artifact_role,
                    error_message: "collection_count must be positive".to_string(),
                },
            );
        }
        if self.metric_count == 0 {
            return Err(
                CalibrationArtifactManifestError::InvalidValidationMetricSummary {
                    artifact_role: self.artifact_role,
                    error_message: "metric_count must be positive".to_string(),
                },
            );
        }

        validate_status_counts(
            self.artifact_role,
            "collection_status_counts",
            self.collection_count,
            self.collection_status_counts.as_slice(),
        )?;
        validate_status_counts(
            self.artifact_role,
            "metric_status_counts",
            self.metric_count,
            self.metric_status_counts.as_slice(),
        )?;

        let mut failing_metric_ids = BTreeSet::new();
        for metric_id in &self.failing_metric_ids {
            if metric_id.trim().is_empty() {
                return Err(
                    CalibrationArtifactManifestError::InvalidValidationMetricSummary {
                        artifact_role: self.artifact_role,
                        error_message: "failing_metric_ids entries must be non-empty".to_string(),
                    },
                );
            }
            if !failing_metric_ids.insert(metric_id.clone()) {
                return Err(
                    CalibrationArtifactManifestError::InvalidValidationMetricSummary {
                        artifact_role: self.artifact_role,
                        error_message: format!(
                            "failing_metric_ids contains duplicate metric_id `{metric_id}`"
                        ),
                    },
                );
            }
        }

        Ok(())
    }
}

/// Referenced persisted artifact entry recorded in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationArtifactReference {
    pub artifact_id: String,
    pub artifact_kind: CalibrationArtifactKind,
    pub artifact_reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<CalibrationStage>,
}

/// High-level artifact categories linked from one persisted run manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationArtifactKind {
    RunContract,
    DatasetSnapshot,
    StageOutput,
    StageCheckpoint,
    Diagnostics,
    Comparison,
    Metadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationArtifactManifestError {
    InvalidSchemaVersionFormat {
        found: String,
    },
    UnexpectedSchemaFamily {
        expected: String,
        found: String,
    },
    UnsupportedSchemaGeneration {
        expected: String,
        found: String,
    },
    MissingRunId,
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    MissingIndexMetadataField {
        field: &'static str,
    },
    InvalidCommittedAtTimestamp {
        found: String,
    },
    CommittedAtTimestampMismatch {
        committed_at_utc: String,
        committed_at_unix_nanos: i64,
        expected_unix_nanos: i64,
    },
    InvalidChecksumFormat {
        field: &'static str,
        found: String,
    },
    ReferenceChecksumMismatch {
        field: &'static str,
        expected: String,
        found: String,
    },
    OptionalIndexMetadataPairMismatch {
        first_field: &'static str,
        second_field: &'static str,
    },
    DuplicateValidationMetricSummaryRole {
        artifact_role: CalibrationArtifactManifestValidationMetricRole,
    },
    InvalidValidationMetricSummary {
        artifact_role: CalibrationArtifactManifestValidationMetricRole,
        error_message: String,
    },
    MissingArtifactReferences,
    EmptyArtifactId,
    EmptyArtifactReference {
        artifact_id: String,
    },
    DuplicateArtifactId {
        artifact_id: String,
    },
}

impl Display for CalibrationArtifactManifestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersionFormat { found } => write!(
                formatter,
                "artifact manifest schema version `{found}` does not match the expected `<schema-family>.<generation>` format"
            ),
            Self::UnexpectedSchemaFamily { expected, found } => write!(
                formatter,
                "artifact manifest schema family `{found}` is unsupported, expected `{expected}`"
            ),
            Self::UnsupportedSchemaGeneration { expected, found } => write!(
                formatter,
                "artifact manifest schema generation `{found}` is unsupported, expected `{expected}`"
            ),
            Self::MissingRunId => write!(formatter, "run_id is required"),
            Self::UnsupportedExecutionMode { expected, found } => write!(
                formatter,
                "artifact manifest requires execution_mode={expected:?}, found `{found:?}`"
            ),
            Self::MissingIndexMetadataField { field } => {
                write!(formatter, "{field} is required")
            }
            Self::InvalidCommittedAtTimestamp { found } => write!(
                formatter,
                "index_metadata.committed_at_utc must be an RFC3339 timestamp, found `{found}`"
            ),
            Self::CommittedAtTimestampMismatch {
                committed_at_utc,
                committed_at_unix_nanos,
                expected_unix_nanos,
            } => write!(
                formatter,
                "index_metadata committed timestamp `{committed_at_utc}` maps to unix nanos `{expected_unix_nanos}`, found `{committed_at_unix_nanos}`"
            ),
            Self::InvalidChecksumFormat { field, found } => write!(
                formatter,
                "{field} must be a lowercase fnv1a_64 hex string, found `{found}`"
            ),
            Self::ReferenceChecksumMismatch {
                field,
                expected,
                found,
            } => write!(
                formatter,
                "{field} must match the referenced dataset checksum `{expected}`, found `{found}`"
            ),
            Self::OptionalIndexMetadataPairMismatch {
                first_field,
                second_field,
            } => write!(
                formatter,
                "{first_field} and {second_field} must either both be present or both be omitted"
            ),
            Self::DuplicateValidationMetricSummaryRole { artifact_role } => write!(
                formatter,
                "index_metadata.validation_metric_summaries contains duplicate artifact_role `{artifact_role:?}`"
            ),
            Self::InvalidValidationMetricSummary {
                artifact_role,
                error_message,
            } => write!(
                formatter,
                "validation metric summary for `{artifact_role:?}` is invalid: {error_message}"
            ),
            Self::MissingArtifactReferences => write!(formatter, "artifact_references is required"),
            Self::EmptyArtifactId => write!(
                formatter,
                "artifact_references entries must use a non-empty artifact_id"
            ),
            Self::EmptyArtifactReference { artifact_id } => write!(
                formatter,
                "artifact_references entry `{artifact_id}` must use a non-empty artifact_reference"
            ),
            Self::DuplicateArtifactId { artifact_id } => write!(
                formatter,
                "artifact_references contains duplicate artifact_id `{artifact_id}`"
            ),
        }
    }
}

impl Error for CalibrationArtifactManifestError {}

fn validate_status_counts(
    artifact_role: CalibrationArtifactManifestValidationMetricRole,
    field_name: &'static str,
    expected_total: usize,
    status_counts: &[CalibrationArtifactManifestMetricStatusCount],
) -> Result<(), CalibrationArtifactManifestError> {
    if status_counts.is_empty() {
        return Err(
            CalibrationArtifactManifestError::InvalidValidationMetricSummary {
                artifact_role,
                error_message: format!("{field_name} must contain at least one status count"),
            },
        );
    }

    let mut seen_statuses = BTreeSet::new();
    let mut total_count = 0usize;
    for status_count in status_counts {
        if status_count.count == 0 {
            return Err(
                CalibrationArtifactManifestError::InvalidValidationMetricSummary {
                    artifact_role,
                    error_message: format!("{field_name} counts must be positive"),
                },
            );
        }

        if !seen_statuses.insert(status_count.status) {
            return Err(
                CalibrationArtifactManifestError::InvalidValidationMetricSummary {
                    artifact_role,
                    error_message: format!(
                        "{field_name} contains duplicate status `{:?}`",
                        status_count.status
                    ),
                },
            );
        }

        total_count = total_count.saturating_add(status_count.count);
    }

    if total_count != expected_total {
        return Err(
            CalibrationArtifactManifestError::InvalidValidationMetricSummary {
                artifact_role,
                error_message: format!(
                    "{field_name} counts sum to `{total_count}`, expected `{expected_total}`"
                ),
            },
        );
    }

    Ok(())
}

fn validate_checksum_hex(
    field: &'static str,
    checksum_hex: &str,
) -> Result<(), CalibrationArtifactManifestError> {
    let is_valid_checksum = checksum_hex.len() == 16
        && checksum_hex
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character));

    if !is_valid_checksum {
        return Err(CalibrationArtifactManifestError::InvalidChecksumFormat {
            field,
            found: checksum_hex.to_string(),
        });
    }

    Ok(())
}

fn validate_reference_checksum(
    field: &'static str,
    data_reference: &str,
    checksum_hex: &str,
) -> Result<(), CalibrationArtifactManifestError> {
    validate_checksum_hex(field, checksum_hex)?;

    let expected_checksum = format!("{:016x}", fnv1a_64(data_reference.as_bytes()));
    if checksum_hex != expected_checksum {
        return Err(
            CalibrationArtifactManifestError::ReferenceChecksumMismatch {
                field,
                expected: expected_checksum,
                found: checksum_hex.to_string(),
            },
        );
    }

    Ok(())
}

fn fnv1a_64(payload: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = FNV_OFFSET;
    for byte in payload {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn validate_artifact_manifest_schema_version(
    schema_version: &str,
) -> Result<(), CalibrationArtifactManifestError> {
    let Some((schema_family, schema_generation_number)) = schema_version.rsplit_once(".v") else {
        return Err(
            CalibrationArtifactManifestError::InvalidSchemaVersionFormat {
                found: schema_version.to_string(),
            },
        );
    };

    if schema_family != CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY {
        return Err(CalibrationArtifactManifestError::UnexpectedSchemaFamily {
            expected: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY.to_string(),
            found: schema_family.to_string(),
        });
    }

    let schema_generation = format!("v{schema_generation_number}");
    if schema_generation != CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION {
        return Err(
            CalibrationArtifactManifestError::UnsupportedSchemaGeneration {
                expected: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION.to_string(),
                found: schema_generation,
            },
        );
    }

    Ok(())
}
