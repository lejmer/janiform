use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::super::contracts::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CalibrationArtifactKind,
    CalibrationArtifactManifest, CalibrationExecutionMode, CalibrationStage,
};
use super::dataset_snapshot::CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY;
use super::manifest::calibration_artifact_manifest_reference;
use super::shared::{
    CalibrationArtifactIoError, CalibrationArtifactPersistenceKind, load_validated_json_artifact,
    persist_validated_json_artifact,
};

pub const CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_VERSION: &str =
    "calibration.artifact_registry.validation_metric.v1";
pub const CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_FAMILY: &str =
    "calibration.artifact_registry.validation_metric";
pub const CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_GENERATION: &str = "v1";
pub const CALIBRATION_VALIDATION_METRIC_ARTIFACT_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "calibration.artifact_registry.validation_metric.output_schema_metadata.v1";

const CALIBRATION_VALIDATION_METRIC_REQUIRED_ARTIFACT_FIELDS: [&str; 10] = [
    "schema_version",
    "run_id",
    "execution_mode",
    "artifact_role",
    "artifact_reference",
    "schema_metadata",
    "output_schema_metadata",
    "manifest_link",
    "stage_output_links",
    "metric_collections",
];
const CALIBRATION_VALIDATION_METRIC_REQUIRED_SCHEMA_METADATA_FIELDS: [&str; 3] = [
    "schema_family",
    "schema_generation",
    "forward_extension_policy",
];
const CALIBRATION_VALIDATION_METRIC_REQUIRED_MANIFEST_LINK_FIELDS: [&str; 2] =
    ["artifact_reference", "schema_version"];
const CALIBRATION_VALIDATION_METRIC_REQUIRED_STAGE_OUTPUT_LINK_FIELDS: [&str; 3] =
    ["stage", "artifact_reference", "stage_output_schema_version"];
const CALIBRATION_VALIDATION_METRIC_REQUIRED_COLLECTION_FIELDS: [&str; 8] = [
    "collection_id",
    "source_kind",
    "source_schema_version",
    "status",
    "context",
    "primary_role_label",
    "threshold_policy_id",
    "metrics",
];
const CALIBRATION_VALIDATION_METRIC_REQUIRED_CONTEXT_FIELDS: [&str; 3] =
    ["context_kind", "context_id", "context_label"];
const CALIBRATION_VALIDATION_METRIC_REQUIRED_METRIC_FIELDS: [&str; 10] = [
    "metric_id",
    "metric_name",
    "metric_variant",
    "status",
    "primary_value",
    "secondary_value",
    "delta_value",
    "absolute_delta",
    "standardized_delta",
    "threshold_metadata",
];
const CALIBRATION_VALIDATION_METRIC_REQUIRED_OBSERVED_VALUE_FIELDS: [&str; 2] =
    ["value", "confidence_interval"];
const CALIBRATION_VALIDATION_METRIC_REQUIRED_THRESHOLD_FIELDS: [&str; 7] = [
    "threshold_policy_id",
    "threshold_policy_version",
    "threshold_id",
    "threshold_rationale",
    "max_absolute_delta",
    "max_standardized_delta",
    "absolute_delta_exceeds_threshold",
];
const CALIBRATION_VALIDATION_METRIC_ARTIFACT_ROLE_VOCABULARY: [&str; 2] =
    ["diagnostics", "comparison"];
const CALIBRATION_VALIDATION_METRIC_STATUS_VOCABULARY: [&str; 6] = [
    "pass",
    "fail",
    "insufficient_data",
    "available",
    "missing",
    "error",
];
const CALIBRATION_VALIDATION_METRIC_SOURCE_KIND_VOCABULARY: [&str; 6] = [
    "activity_clustering",
    "tail_skew",
    "volatility_persistence",
    "gap_behavior",
    "stylized_fact_horizon_comparison",
    "scale_consistency",
];
const CALIBRATION_VALIDATION_METRIC_CONTEXT_KIND_VOCABULARY: [&str; 5] =
    ["run", "batch", "horizon", "scenario", "stage"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationValidationMetricArtifactRole {
    Diagnostics,
    Comparison,
}

impl CalibrationValidationMetricArtifactRole {
    pub const fn artifact_kind(self) -> CalibrationArtifactKind {
        match self {
            Self::Diagnostics => CalibrationArtifactKind::Diagnostics,
            Self::Comparison => CalibrationArtifactKind::Comparison,
        }
    }

    pub const fn artifact_token(self) -> &'static str {
        match self {
            Self::Diagnostics => "diagnostics",
            Self::Comparison => "comparison",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationValidationMetricStatus {
    Pass,
    Fail,
    InsufficientData,
    Available,
    Missing,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationValidationMetricSourceKind {
    ActivityClustering,
    TailSkew,
    VolatilityPersistence,
    GapBehavior,
    StylizedFactHorizonComparison,
    ScaleConsistency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationValidationMetricContextKind {
    Run,
    Batch,
    Horizon,
    Scenario,
    Stage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationValidationMetricSchemaMetadata {
    pub schema_family: String,
    pub schema_generation: String,
    pub forward_extension_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationValidationMetricOutputSchemaMetadata {
    pub schema_version: String,
    pub required_artifact_fields: Vec<String>,
    pub required_schema_metadata_fields: Vec<String>,
    pub required_manifest_link_fields: Vec<String>,
    pub required_stage_output_link_fields: Vec<String>,
    pub required_collection_fields: Vec<String>,
    pub required_context_fields: Vec<String>,
    pub required_metric_fields: Vec<String>,
    pub required_observed_value_fields: Vec<String>,
    pub required_threshold_fields: Vec<String>,
    pub artifact_role_vocabulary: Vec<String>,
    pub status_vocabulary: Vec<String>,
    pub source_kind_vocabulary: Vec<String>,
    pub context_kind_vocabulary: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationValidationMetricManifestLink {
    pub artifact_reference: String,
    pub schema_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationValidationMetricStageOutputLink {
    pub stage: CalibrationStage,
    pub artifact_reference: String,
    pub stage_output_schema_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationValidationMetricContext {
    pub context_kind: CalibrationValidationMetricContextKind,
    pub context_id: String,
    pub context_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationValidationMetricConfidenceInterval {
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub confidence_level: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationValidationObservedValue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_interval: Option<CalibrationValidationMetricConfidenceInterval>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationValidationMetricThresholdMetadata {
    pub threshold_policy_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_policy_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_absolute_delta: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_standardized_delta: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_delta_exceeds_threshold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standardized_delta_exceeds_threshold: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationValidationMetricRecord {
    pub metric_id: String,
    pub metric_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_variant: Option<String>,
    pub status: CalibrationValidationMetricStatus,
    pub primary_value: CalibrationValidationObservedValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_value: Option<CalibrationValidationObservedValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_delta: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standardized_delta: Option<f64>,
    pub threshold_metadata: CalibrationValidationMetricThresholdMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationValidationMetricCollection {
    pub collection_id: String,
    pub source_kind: CalibrationValidationMetricSourceKind,
    pub source_schema_version: String,
    pub status: CalibrationValidationMetricStatus,
    pub context: CalibrationValidationMetricContext,
    pub primary_role_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_role_label: Option<String>,
    pub threshold_policy_id: String,
    pub metrics: Vec<CalibrationValidationMetricRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationValidationMetricArtifact {
    pub schema_version: String,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub artifact_role: CalibrationValidationMetricArtifactRole,
    pub artifact_reference: String,
    pub schema_metadata: CalibrationValidationMetricSchemaMetadata,
    pub output_schema_metadata: CalibrationValidationMetricOutputSchemaMetadata,
    pub manifest_link: CalibrationValidationMetricManifestLink,
    pub stage_output_links: Vec<CalibrationValidationMetricStageOutputLink>,
    pub metric_collections: Vec<CalibrationValidationMetricCollection>,
}

impl CalibrationValidationMetricArtifact {
    pub fn try_new(
        run_id: impl Into<String>,
        artifact_role: CalibrationValidationMetricArtifactRole,
        stage_output_links: Vec<CalibrationValidationMetricStageOutputLink>,
        metric_collections: Vec<CalibrationValidationMetricCollection>,
    ) -> Result<Self, CalibrationValidationMetricArtifactValidationError> {
        let run_id = run_id.into();
        let artifact = Self {
            schema_version: CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_VERSION.to_string(),
            run_id: run_id.clone(),
            execution_mode: CalibrationExecutionMode::Disk,
            artifact_role,
            artifact_reference: calibration_validation_metric_artifact_reference(
                run_id.as_str(),
                artifact_role,
            ),
            schema_metadata: CalibrationValidationMetricSchemaMetadata {
                schema_family: CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_FAMILY.to_string(),
                schema_generation: CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_GENERATION
                    .to_string(),
                forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                    .to_string(),
            },
            output_schema_metadata: build_output_schema_metadata(),
            manifest_link: CalibrationValidationMetricManifestLink {
                artifact_reference: calibration_artifact_manifest_reference(run_id.as_str()),
                schema_version: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
            },
            stage_output_links,
            metric_collections,
        };

        artifact.validate()?;

        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), CalibrationValidationMetricArtifactValidationError> {
        validate_exact_string(
            "schema_version",
            CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_VERSION,
            self.schema_version.as_str(),
        )?;

        if self.run_id.trim().is_empty() {
            return Err(CalibrationValidationMetricArtifactValidationError::EmptyRunId);
        }
        if self.execution_mode != CalibrationExecutionMode::Disk {
            return Err(
                CalibrationValidationMetricArtifactValidationError::UnsupportedExecutionMode {
                    expected: CalibrationExecutionMode::Disk,
                    found: self.execution_mode,
                },
            );
        }
        if self.artifact_reference.trim().is_empty() {
            return Err(CalibrationValidationMetricArtifactValidationError::EmptyArtifactReference);
        }

        validate_exact_string(
            "schema_metadata.schema_family",
            CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_FAMILY,
            self.schema_metadata.schema_family.as_str(),
        )?;
        validate_exact_string(
            "schema_metadata.schema_generation",
            CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_GENERATION,
            self.schema_metadata.schema_generation.as_str(),
        )?;
        validate_exact_string(
            "schema_metadata.forward_extension_policy",
            CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY,
            self.schema_metadata.forward_extension_policy.as_str(),
        )?;

        validate_exact_string(
            "output_schema_metadata.schema_version",
            CALIBRATION_VALIDATION_METRIC_ARTIFACT_OUTPUT_SCHEMA_METADATA_VERSION,
            self.output_schema_metadata.schema_version.as_str(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.required_artifact_fields",
            &CALIBRATION_VALIDATION_METRIC_REQUIRED_ARTIFACT_FIELDS,
            self.output_schema_metadata
                .required_artifact_fields
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.required_schema_metadata_fields",
            &CALIBRATION_VALIDATION_METRIC_REQUIRED_SCHEMA_METADATA_FIELDS,
            self.output_schema_metadata
                .required_schema_metadata_fields
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.required_manifest_link_fields",
            &CALIBRATION_VALIDATION_METRIC_REQUIRED_MANIFEST_LINK_FIELDS,
            self.output_schema_metadata
                .required_manifest_link_fields
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.required_stage_output_link_fields",
            &CALIBRATION_VALIDATION_METRIC_REQUIRED_STAGE_OUTPUT_LINK_FIELDS,
            self.output_schema_metadata
                .required_stage_output_link_fields
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.required_collection_fields",
            &CALIBRATION_VALIDATION_METRIC_REQUIRED_COLLECTION_FIELDS,
            self.output_schema_metadata
                .required_collection_fields
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.required_context_fields",
            &CALIBRATION_VALIDATION_METRIC_REQUIRED_CONTEXT_FIELDS,
            self.output_schema_metadata
                .required_context_fields
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.required_metric_fields",
            &CALIBRATION_VALIDATION_METRIC_REQUIRED_METRIC_FIELDS,
            self.output_schema_metadata
                .required_metric_fields
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.required_observed_value_fields",
            &CALIBRATION_VALIDATION_METRIC_REQUIRED_OBSERVED_VALUE_FIELDS,
            self.output_schema_metadata
                .required_observed_value_fields
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.required_threshold_fields",
            &CALIBRATION_VALIDATION_METRIC_REQUIRED_THRESHOLD_FIELDS,
            self.output_schema_metadata
                .required_threshold_fields
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.artifact_role_vocabulary",
            &CALIBRATION_VALIDATION_METRIC_ARTIFACT_ROLE_VOCABULARY,
            self.output_schema_metadata
                .artifact_role_vocabulary
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.status_vocabulary",
            &CALIBRATION_VALIDATION_METRIC_STATUS_VOCABULARY,
            self.output_schema_metadata.status_vocabulary.as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.source_kind_vocabulary",
            &CALIBRATION_VALIDATION_METRIC_SOURCE_KIND_VOCABULARY,
            self.output_schema_metadata
                .source_kind_vocabulary
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.context_kind_vocabulary",
            &CALIBRATION_VALIDATION_METRIC_CONTEXT_KIND_VOCABULARY,
            self.output_schema_metadata
                .context_kind_vocabulary
                .as_slice(),
        )?;

        if self.manifest_link.artifact_reference.trim().is_empty() {
            return Err(CalibrationValidationMetricArtifactValidationError::EmptyManifestReference);
        }
        validate_exact_string(
            "manifest_link.schema_version",
            CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION,
            self.manifest_link.schema_version.as_str(),
        )?;

        if self.stage_output_links.is_empty() {
            return Err(
                CalibrationValidationMetricArtifactValidationError::MissingStageOutputLinks,
            );
        }

        let mut seen_stage_links = BTreeSet::new();
        for stage_output_link in &self.stage_output_links {
            if stage_output_link.artifact_reference.trim().is_empty() {
                return Err(
                    CalibrationValidationMetricArtifactValidationError::EmptyStageOutputReference {
                        stage: stage_output_link.stage,
                    },
                );
            }

            if stage_output_link
                .stage_output_schema_version
                .trim()
                .is_empty()
            {
                return Err(CalibrationValidationMetricArtifactValidationError::EmptyStageOutputSchemaVersion {
                    stage: stage_output_link.stage,
                });
            }

            if !seen_stage_links.insert((
                stage_output_link.stage,
                stage_output_link.artifact_reference.clone(),
            )) {
                return Err(
                    CalibrationValidationMetricArtifactValidationError::DuplicateStageOutputLink {
                        stage: stage_output_link.stage,
                        artifact_reference: stage_output_link.artifact_reference.clone(),
                    },
                );
            }
        }

        if self.metric_collections.is_empty() {
            return Err(
                CalibrationValidationMetricArtifactValidationError::MissingMetricCollections,
            );
        }

        let mut collection_ids = BTreeSet::new();
        for collection in &self.metric_collections {
            validate_collection(collection)?;
            if !collection_ids.insert(collection.collection_id.clone()) {
                return Err(
                    CalibrationValidationMetricArtifactValidationError::DuplicateCollectionId {
                        collection_id: collection.collection_id.clone(),
                    },
                );
            }
        }

        Ok(())
    }

    pub fn validate_against_manifest(
        &self,
        manifest: &CalibrationArtifactManifest,
    ) -> Result<(), CalibrationValidationMetricArtifactReferenceIntegrityError> {
        self.validate()
            .map_err(CalibrationValidationMetricArtifactReferenceIntegrityError::InvalidArtifact)?;
        manifest
            .validate()
            .map_err(CalibrationValidationMetricArtifactReferenceIntegrityError::InvalidManifest)?;

        if self.run_id != manifest.run_id {
            return Err(
                CalibrationValidationMetricArtifactReferenceIntegrityError::RunIdMismatch {
                    artifact_run_id: self.run_id.clone(),
                    manifest_run_id: manifest.run_id.clone(),
                },
            );
        }
        if self.manifest_link.artifact_reference
            != calibration_artifact_manifest_reference(self.run_id.as_str())
        {
            return Err(
                CalibrationValidationMetricArtifactReferenceIntegrityError::UnexpectedManifestReference {
                    expected: calibration_artifact_manifest_reference(self.run_id.as_str()),
                    found: self.manifest_link.artifact_reference.clone(),
                },
            );
        }

        for stage_output_link in &self.stage_output_links {
            let linked_reference = manifest
                .artifact_references
                .iter()
                .find(|artifact_reference| {
                    artifact_reference.artifact_kind == CalibrationArtifactKind::StageOutput
                        && artifact_reference.stage == Some(stage_output_link.stage)
                        && artifact_reference.artifact_reference
                            == stage_output_link.artifact_reference
                });
            if linked_reference.is_none() {
                return Err(
                    CalibrationValidationMetricArtifactReferenceIntegrityError::UnknownStageOutputReference {
                        stage: stage_output_link.stage,
                        artifact_reference: stage_output_link.artifact_reference.clone(),
                    },
                );
            }
        }

        Ok(())
    }
}

pub fn calibration_validation_metric_artifact_reference(
    run_id: &str,
    artifact_role: CalibrationValidationMetricArtifactRole,
) -> String {
    format!("artifact://{}/{}", artifact_role.artifact_token(), run_id)
}

pub fn calibration_validation_metric_artifact_path(
    disk_checkpoint_directory: &Path,
    run_id: &str,
    artifact_role: CalibrationValidationMetricArtifactRole,
) -> PathBuf {
    disk_checkpoint_directory.join(run_id).join(format!(
        "{}.validation_metric.artifact.json",
        artifact_role.artifact_token()
    ))
}

pub fn persist_calibration_validation_metric_artifact_to_disk(
    disk_checkpoint_directory: &Path,
    artifact: &CalibrationValidationMetricArtifact,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::ValidationMetric;
    let artifact_path = calibration_validation_metric_artifact_path(
        disk_checkpoint_directory,
        artifact.run_id.as_str(),
        artifact.artifact_role,
    );
    persist_validated_json_artifact(
        artifact_kind,
        artifact_path.as_path(),
        artifact,
        |payload| payload.validate().map_err(|source| source.to_string()),
    )
}

pub fn load_calibration_validation_metric_artifact_from_disk(
    artifact_path: &Path,
) -> Result<CalibrationValidationMetricArtifact, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::ValidationMetric;
    load_validated_json_artifact(
        artifact_kind,
        artifact_path,
        |payload: &CalibrationValidationMetricArtifact| {
            payload.validate().map_err(|source| source.to_string())
        },
    )
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationValidationMetricArtifactValidationError {
    EmptyRunId,
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    EmptyArtifactReference,
    InvalidMetadataField {
        field: &'static str,
        expected: String,
        found: String,
    },
    InvalidMetadataList {
        field: &'static str,
        expected: Vec<String>,
        found: Vec<String>,
    },
    EmptyManifestReference,
    MissingStageOutputLinks,
    EmptyStageOutputReference {
        stage: CalibrationStage,
    },
    EmptyStageOutputSchemaVersion {
        stage: CalibrationStage,
    },
    DuplicateStageOutputLink {
        stage: CalibrationStage,
        artifact_reference: String,
    },
    MissingMetricCollections,
    DuplicateCollectionId {
        collection_id: String,
    },
    EmptyCollectionId,
    EmptySourceSchemaVersion {
        collection_id: String,
    },
    EmptyContextId {
        collection_id: String,
    },
    EmptyContextLabel {
        collection_id: String,
    },
    EmptyPrimaryRoleLabel {
        collection_id: String,
    },
    EmptySecondaryRoleLabel {
        collection_id: String,
    },
    EmptyThresholdPolicyId {
        collection_id: String,
    },
    MissingMetrics {
        collection_id: String,
    },
    DuplicateMetricKey {
        collection_id: String,
        metric_id: String,
        metric_variant: Option<String>,
    },
    EmptyMetricId {
        collection_id: String,
    },
    EmptyMetricName {
        collection_id: String,
        metric_id: String,
    },
    EmptyMetricVariant {
        collection_id: String,
        metric_id: String,
    },
    InvalidFiniteMetricField {
        collection_id: String,
        metric_id: String,
        field: &'static str,
        value: f64,
    },
    InvalidConfidenceInterval {
        collection_id: String,
        metric_id: String,
        side: &'static str,
        details: String,
    },
    InvalidThresholdMetadata {
        collection_id: String,
        metric_id: String,
        details: String,
    },
    MissingMetricValueForStatus {
        collection_id: String,
        metric_id: String,
        status: CalibrationValidationMetricStatus,
    },
}

impl Display for CalibrationValidationMetricArtifactValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyRunId => write!(formatter, "run_id must be a non-empty string"),
            Self::UnsupportedExecutionMode { expected, found } => write!(
                formatter,
                "validation metric artifact requires execution_mode={expected:?}, found `{found:?}`"
            ),
            Self::EmptyArtifactReference => {
                write!(formatter, "artifact_reference must be a non-empty string")
            }
            Self::InvalidMetadataField {
                field,
                expected,
                found,
            } => write!(formatter, "{field} must be `{expected}`, found `{found}`"),
            Self::InvalidMetadataList {
                field,
                expected,
                found,
            } => write!(
                formatter,
                "{field} must equal {:?}, found {:?}",
                expected, found
            ),
            Self::EmptyManifestReference => write!(
                formatter,
                "manifest_link.artifact_reference must be a non-empty string"
            ),
            Self::MissingStageOutputLinks => write!(
                formatter,
                "stage_output_links must contain at least one linked stage artifact"
            ),
            Self::EmptyStageOutputReference { stage } => write!(
                formatter,
                "stage_output_links entry for stage `{stage:?}` must use a non-empty artifact_reference"
            ),
            Self::EmptyStageOutputSchemaVersion { stage } => write!(
                formatter,
                "stage_output_links entry for stage `{stage:?}` must use a non-empty stage_output_schema_version"
            ),
            Self::DuplicateStageOutputLink {
                stage,
                artifact_reference,
            } => write!(
                formatter,
                "stage_output_links contains duplicate entry for stage `{stage:?}` and artifact_reference `{artifact_reference}`"
            ),
            Self::MissingMetricCollections => {
                write!(
                    formatter,
                    "metric_collections must contain at least one collection"
                )
            }
            Self::DuplicateCollectionId { collection_id } => write!(
                formatter,
                "metric_collections contains duplicate collection_id `{collection_id}`"
            ),
            Self::EmptyCollectionId => write!(
                formatter,
                "metric_collections entries must use a non-empty collection_id"
            ),
            Self::EmptySourceSchemaVersion { collection_id } => write!(
                formatter,
                "metric_collections entry `{collection_id}` must use a non-empty source_schema_version"
            ),
            Self::EmptyContextId { collection_id } => write!(
                formatter,
                "metric_collections entry `{collection_id}` must use a non-empty context.context_id"
            ),
            Self::EmptyContextLabel { collection_id } => write!(
                formatter,
                "metric_collections entry `{collection_id}` must use a non-empty context.context_label"
            ),
            Self::EmptyPrimaryRoleLabel { collection_id } => write!(
                formatter,
                "metric_collections entry `{collection_id}` must use a non-empty primary_role_label"
            ),
            Self::EmptySecondaryRoleLabel { collection_id } => write!(
                formatter,
                "metric_collections entry `{collection_id}` must use a non-empty secondary_role_label when provided"
            ),
            Self::EmptyThresholdPolicyId { collection_id } => write!(
                formatter,
                "metric_collections entry `{collection_id}` must use a non-empty threshold_policy_id"
            ),
            Self::MissingMetrics { collection_id } => write!(
                formatter,
                "metric_collections entry `{collection_id}` must contain at least one metric"
            ),
            Self::DuplicateMetricKey {
                collection_id,
                metric_id,
                metric_variant,
            } => write!(
                formatter,
                "metric_collections entry `{collection_id}` contains duplicate metric_id `{metric_id}` and metric_variant {metric_variant:?}"
            ),
            Self::EmptyMetricId { collection_id } => write!(
                formatter,
                "metric record in collection `{collection_id}` must use a non-empty metric_id"
            ),
            Self::EmptyMetricName {
                collection_id,
                metric_id,
            } => write!(
                formatter,
                "metric `{metric_id}` in collection `{collection_id}` must use a non-empty metric_name"
            ),
            Self::EmptyMetricVariant {
                collection_id,
                metric_id,
            } => write!(
                formatter,
                "metric `{metric_id}` in collection `{collection_id}` must use a non-empty metric_variant when provided"
            ),
            Self::InvalidFiniteMetricField {
                collection_id,
                metric_id,
                field,
                value,
            } => write!(
                formatter,
                "metric `{metric_id}` in collection `{collection_id}` uses invalid `{field}` value {value}"
            ),
            Self::InvalidConfidenceInterval {
                collection_id,
                metric_id,
                side,
                details,
            } => write!(
                formatter,
                "{side} confidence_interval for metric `{metric_id}` in collection `{collection_id}` is invalid: {details}"
            ),
            Self::InvalidThresholdMetadata {
                collection_id,
                metric_id,
                details,
            } => write!(
                formatter,
                "threshold_metadata for metric `{metric_id}` in collection `{collection_id}` is invalid: {details}"
            ),
            Self::MissingMetricValueForStatus {
                collection_id,
                metric_id,
                status,
            } => write!(
                formatter,
                "metric `{metric_id}` in collection `{collection_id}` must include at least one value or delta field for status `{status:?}`"
            ),
        }
    }
}

impl Error for CalibrationValidationMetricArtifactValidationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationValidationMetricArtifactReferenceIntegrityError {
    InvalidArtifact(CalibrationValidationMetricArtifactValidationError),
    InvalidManifest(super::super::contracts::CalibrationArtifactManifestError),
    RunIdMismatch {
        artifact_run_id: String,
        manifest_run_id: String,
    },
    UnexpectedManifestReference {
        expected: String,
        found: String,
    },
    UnknownStageOutputReference {
        stage: CalibrationStage,
        artifact_reference: String,
    },
}

impl Display for CalibrationValidationMetricArtifactReferenceIntegrityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArtifact(source) => write!(formatter, "artifact is invalid: {source}"),
            Self::InvalidManifest(source) => write!(formatter, "manifest is invalid: {source}"),
            Self::RunIdMismatch {
                artifact_run_id,
                manifest_run_id,
            } => write!(
                formatter,
                "artifact run_id `{artifact_run_id}` does not match manifest run_id `{manifest_run_id}`"
            ),
            Self::UnexpectedManifestReference { expected, found } => write!(
                formatter,
                "manifest_link.artifact_reference must be `{expected}`, found `{found}`"
            ),
            Self::UnknownStageOutputReference {
                stage,
                artifact_reference,
            } => write!(
                formatter,
                "stage_output_links entry for stage `{stage:?}` with artifact_reference `{artifact_reference}` is not present in the manifest"
            ),
        }
    }
}

impl Error for CalibrationValidationMetricArtifactReferenceIntegrityError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidArtifact(source) => Some(source),
            Self::InvalidManifest(source) => Some(source),
            _ => None,
        }
    }
}

fn build_output_schema_metadata() -> CalibrationValidationMetricOutputSchemaMetadata {
    CalibrationValidationMetricOutputSchemaMetadata {
        schema_version: CALIBRATION_VALIDATION_METRIC_ARTIFACT_OUTPUT_SCHEMA_METADATA_VERSION
            .to_string(),
        required_artifact_fields: CALIBRATION_VALIDATION_METRIC_REQUIRED_ARTIFACT_FIELDS
            .iter()
            .map(ToString::to_string)
            .collect(),
        required_schema_metadata_fields:
            CALIBRATION_VALIDATION_METRIC_REQUIRED_SCHEMA_METADATA_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect(),
        required_manifest_link_fields: CALIBRATION_VALIDATION_METRIC_REQUIRED_MANIFEST_LINK_FIELDS
            .iter()
            .map(ToString::to_string)
            .collect(),
        required_stage_output_link_fields:
            CALIBRATION_VALIDATION_METRIC_REQUIRED_STAGE_OUTPUT_LINK_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect(),
        required_collection_fields: CALIBRATION_VALIDATION_METRIC_REQUIRED_COLLECTION_FIELDS
            .iter()
            .map(ToString::to_string)
            .collect(),
        required_context_fields: CALIBRATION_VALIDATION_METRIC_REQUIRED_CONTEXT_FIELDS
            .iter()
            .map(ToString::to_string)
            .collect(),
        required_metric_fields: CALIBRATION_VALIDATION_METRIC_REQUIRED_METRIC_FIELDS
            .iter()
            .map(ToString::to_string)
            .collect(),
        required_observed_value_fields:
            CALIBRATION_VALIDATION_METRIC_REQUIRED_OBSERVED_VALUE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect(),
        required_threshold_fields: CALIBRATION_VALIDATION_METRIC_REQUIRED_THRESHOLD_FIELDS
            .iter()
            .map(ToString::to_string)
            .collect(),
        artifact_role_vocabulary: CALIBRATION_VALIDATION_METRIC_ARTIFACT_ROLE_VOCABULARY
            .iter()
            .map(ToString::to_string)
            .collect(),
        status_vocabulary: CALIBRATION_VALIDATION_METRIC_STATUS_VOCABULARY
            .iter()
            .map(ToString::to_string)
            .collect(),
        source_kind_vocabulary: CALIBRATION_VALIDATION_METRIC_SOURCE_KIND_VOCABULARY
            .iter()
            .map(ToString::to_string)
            .collect(),
        context_kind_vocabulary: CALIBRATION_VALIDATION_METRIC_CONTEXT_KIND_VOCABULARY
            .iter()
            .map(ToString::to_string)
            .collect(),
    }
}

fn validate_collection(
    collection: &CalibrationValidationMetricCollection,
) -> Result<(), CalibrationValidationMetricArtifactValidationError> {
    if collection.collection_id.trim().is_empty() {
        return Err(CalibrationValidationMetricArtifactValidationError::EmptyCollectionId);
    }
    if collection.source_schema_version.trim().is_empty() {
        return Err(
            CalibrationValidationMetricArtifactValidationError::EmptySourceSchemaVersion {
                collection_id: collection.collection_id.clone(),
            },
        );
    }
    if collection.context.context_id.trim().is_empty() {
        return Err(
            CalibrationValidationMetricArtifactValidationError::EmptyContextId {
                collection_id: collection.collection_id.clone(),
            },
        );
    }
    if collection.context.context_label.trim().is_empty() {
        return Err(
            CalibrationValidationMetricArtifactValidationError::EmptyContextLabel {
                collection_id: collection.collection_id.clone(),
            },
        );
    }
    if collection.primary_role_label.trim().is_empty() {
        return Err(
            CalibrationValidationMetricArtifactValidationError::EmptyPrimaryRoleLabel {
                collection_id: collection.collection_id.clone(),
            },
        );
    }
    if collection
        .secondary_role_label
        .as_deref()
        .is_some_and(|label| label.trim().is_empty())
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::EmptySecondaryRoleLabel {
                collection_id: collection.collection_id.clone(),
            },
        );
    }
    if collection.threshold_policy_id.trim().is_empty() {
        return Err(
            CalibrationValidationMetricArtifactValidationError::EmptyThresholdPolicyId {
                collection_id: collection.collection_id.clone(),
            },
        );
    }
    if collection.metrics.is_empty() {
        return Err(
            CalibrationValidationMetricArtifactValidationError::MissingMetrics {
                collection_id: collection.collection_id.clone(),
            },
        );
    }

    let mut metric_keys = BTreeSet::new();
    for metric in &collection.metrics {
        validate_metric_record(collection, metric)?;
        if !metric_keys.insert((metric.metric_id.clone(), metric.metric_variant.clone())) {
            return Err(
                CalibrationValidationMetricArtifactValidationError::DuplicateMetricKey {
                    collection_id: collection.collection_id.clone(),
                    metric_id: metric.metric_id.clone(),
                    metric_variant: metric.metric_variant.clone(),
                },
            );
        }
    }

    Ok(())
}

fn validate_metric_record(
    collection: &CalibrationValidationMetricCollection,
    metric: &CalibrationValidationMetricRecord,
) -> Result<(), CalibrationValidationMetricArtifactValidationError> {
    if metric.metric_id.trim().is_empty() {
        return Err(
            CalibrationValidationMetricArtifactValidationError::EmptyMetricId {
                collection_id: collection.collection_id.clone(),
            },
        );
    }
    if metric.metric_name.trim().is_empty() {
        return Err(
            CalibrationValidationMetricArtifactValidationError::EmptyMetricName {
                collection_id: collection.collection_id.clone(),
                metric_id: metric.metric_id.clone(),
            },
        );
    }
    if metric
        .metric_variant
        .as_deref()
        .is_some_and(|variant| variant.trim().is_empty())
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::EmptyMetricVariant {
                collection_id: collection.collection_id.clone(),
                metric_id: metric.metric_id.clone(),
            },
        );
    }

    validate_observed_value(
        collection.collection_id.as_str(),
        metric.metric_id.as_str(),
        "primary_value",
        &metric.primary_value,
    )?;
    if let Some(secondary_value) = &metric.secondary_value {
        validate_observed_value(
            collection.collection_id.as_str(),
            metric.metric_id.as_str(),
            "secondary_value",
            secondary_value,
        )?;
    }

    validate_optional_finite(
        collection.collection_id.as_str(),
        metric.metric_id.as_str(),
        "delta_value",
        metric.delta_value,
    )?;
    validate_optional_non_negative_finite(
        collection.collection_id.as_str(),
        metric.metric_id.as_str(),
        "absolute_delta",
        metric.absolute_delta,
    )?;
    validate_optional_non_negative_finite(
        collection.collection_id.as_str(),
        metric.metric_id.as_str(),
        "standardized_delta",
        metric.standardized_delta,
    )?;
    validate_threshold_metadata(
        collection.collection_id.as_str(),
        metric.metric_id.as_str(),
        collection.threshold_policy_id.as_str(),
        &metric.threshold_metadata,
    )?;

    let has_value_or_delta = metric.primary_value.value.is_some()
        || metric
            .secondary_value
            .as_ref()
            .and_then(|value| value.value)
            .is_some()
        || metric.delta_value.is_some()
        || metric.absolute_delta.is_some()
        || metric.standardized_delta.is_some();
    if !has_value_or_delta
        && matches!(
            metric.status,
            CalibrationValidationMetricStatus::Pass
                | CalibrationValidationMetricStatus::Fail
                | CalibrationValidationMetricStatus::Available
        )
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::MissingMetricValueForStatus {
                collection_id: collection.collection_id.clone(),
                metric_id: metric.metric_id.clone(),
                status: metric.status,
            },
        );
    }

    Ok(())
}

fn validate_observed_value(
    collection_id: &str,
    metric_id: &str,
    side: &'static str,
    observed_value: &CalibrationValidationObservedValue,
) -> Result<(), CalibrationValidationMetricArtifactValidationError> {
    if let Some(value) = observed_value.value
        && !value.is_finite()
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidFiniteMetricField {
                collection_id: collection_id.to_string(),
                metric_id: metric_id.to_string(),
                field: side,
                value,
            },
        );
    }

    if let Some(confidence_interval) = observed_value.confidence_interval {
        if !confidence_interval.lower_bound.is_finite()
            || !confidence_interval.upper_bound.is_finite()
        {
            return Err(
                CalibrationValidationMetricArtifactValidationError::InvalidConfidenceInterval {
                    collection_id: collection_id.to_string(),
                    metric_id: metric_id.to_string(),
                    side,
                    details: "bounds must be finite".to_string(),
                },
            );
        }

        if confidence_interval.lower_bound > confidence_interval.upper_bound {
            return Err(
                CalibrationValidationMetricArtifactValidationError::InvalidConfidenceInterval {
                    collection_id: collection_id.to_string(),
                    metric_id: metric_id.to_string(),
                    side,
                    details: "lower_bound must not exceed upper_bound".to_string(),
                },
            );
        }

        if !confidence_interval.confidence_level.is_finite()
            || confidence_interval.confidence_level <= 0.0
            || confidence_interval.confidence_level >= 1.0
        {
            return Err(
                CalibrationValidationMetricArtifactValidationError::InvalidConfidenceInterval {
                    collection_id: collection_id.to_string(),
                    metric_id: metric_id.to_string(),
                    side,
                    details: "confidence_level must be finite and lie strictly between 0 and 1"
                        .to_string(),
                },
            );
        }
    }

    Ok(())
}

fn validate_threshold_metadata(
    collection_id: &str,
    metric_id: &str,
    collection_threshold_policy_id: &str,
    threshold_metadata: &CalibrationValidationMetricThresholdMetadata,
) -> Result<(), CalibrationValidationMetricArtifactValidationError> {
    if threshold_metadata.threshold_policy_id.trim().is_empty() {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidThresholdMetadata {
                collection_id: collection_id.to_string(),
                metric_id: metric_id.to_string(),
                details: "threshold_policy_id must be non-empty".to_string(),
            },
        );
    }

    if threshold_metadata.threshold_policy_id != collection_threshold_policy_id {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidThresholdMetadata {
                collection_id: collection_id.to_string(),
                metric_id: metric_id.to_string(),
                details: format!(
                    "threshold_policy_id `{}` does not match collection threshold_policy_id `{}`",
                    threshold_metadata.threshold_policy_id, collection_threshold_policy_id
                ),
            },
        );
    }

    if threshold_metadata
        .threshold_policy_version
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidThresholdMetadata {
                collection_id: collection_id.to_string(),
                metric_id: metric_id.to_string(),
                details: "threshold_policy_version must be non-empty when provided".to_string(),
            },
        );
    }

    if threshold_metadata
        .threshold_id
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidThresholdMetadata {
                collection_id: collection_id.to_string(),
                metric_id: metric_id.to_string(),
                details: "threshold_id must be non-empty when provided".to_string(),
            },
        );
    }

    if threshold_metadata
        .threshold_rationale
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidThresholdMetadata {
                collection_id: collection_id.to_string(),
                metric_id: metric_id.to_string(),
                details: "threshold_rationale must be non-empty when provided".to_string(),
            },
        );
    }

    if threshold_metadata.max_absolute_delta.is_none()
        && threshold_metadata.max_standardized_delta.is_none()
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidThresholdMetadata {
                collection_id: collection_id.to_string(),
                metric_id: metric_id.to_string(),
                details: "at least one threshold bound must be provided".to_string(),
            },
        );
    }

    validate_threshold_bound(
        collection_id,
        metric_id,
        "max_absolute_delta",
        threshold_metadata.max_absolute_delta,
    )?;
    validate_threshold_bound(
        collection_id,
        metric_id,
        "max_standardized_delta",
        threshold_metadata.max_standardized_delta,
    )?;

    if threshold_metadata
        .absolute_delta_exceeds_threshold
        .is_some()
        && threshold_metadata.max_absolute_delta.is_none()
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidThresholdMetadata {
                collection_id: collection_id.to_string(),
                metric_id: metric_id.to_string(),
                details:
                    "absolute_delta_exceeds_threshold requires max_absolute_delta to be provided"
                        .to_string(),
            },
        );
    }

    if threshold_metadata
        .standardized_delta_exceeds_threshold
        .is_some()
        && threshold_metadata.max_standardized_delta.is_none()
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidThresholdMetadata {
                collection_id: collection_id.to_string(),
                metric_id: metric_id.to_string(),
                details:
                    "standardized_delta_exceeds_threshold requires max_standardized_delta to be provided"
                        .to_string(),
            },
        );
    }

    Ok(())
}

fn validate_threshold_bound(
    collection_id: &str,
    metric_id: &str,
    field: &'static str,
    value: Option<f64>,
) -> Result<(), CalibrationValidationMetricArtifactValidationError> {
    if let Some(value) = value
        && (!value.is_finite() || value < 0.0)
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidThresholdMetadata {
                collection_id: collection_id.to_string(),
                metric_id: metric_id.to_string(),
                details: format!("{field} must be finite and non-negative"),
            },
        );
    }

    Ok(())
}

fn validate_optional_finite(
    collection_id: &str,
    metric_id: &str,
    field: &'static str,
    value: Option<f64>,
) -> Result<(), CalibrationValidationMetricArtifactValidationError> {
    if let Some(value) = value
        && !value.is_finite()
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidFiniteMetricField {
                collection_id: collection_id.to_string(),
                metric_id: metric_id.to_string(),
                field,
                value,
            },
        );
    }

    Ok(())
}

fn validate_optional_non_negative_finite(
    collection_id: &str,
    metric_id: &str,
    field: &'static str,
    value: Option<f64>,
) -> Result<(), CalibrationValidationMetricArtifactValidationError> {
    if let Some(value) = value
        && (!value.is_finite() || value < 0.0)
    {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidFiniteMetricField {
                collection_id: collection_id.to_string(),
                metric_id: metric_id.to_string(),
                field,
                value,
            },
        );
    }

    Ok(())
}

fn validate_exact_string(
    field: &'static str,
    expected: &str,
    found: &str,
) -> Result<(), CalibrationValidationMetricArtifactValidationError> {
    if found != expected {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidMetadataField {
                field,
                expected: expected.to_string(),
                found: found.to_string(),
            },
        );
    }

    Ok(())
}

fn validate_exact_string_list(
    field: &'static str,
    expected: &[&str],
    found: &[String],
) -> Result<(), CalibrationValidationMetricArtifactValidationError> {
    let expected_values = expected
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<String>>();
    if found != expected_values {
        return Err(
            CalibrationValidationMetricArtifactValidationError::InvalidMetadataList {
                field,
                expected: expected_values,
                found: found.to_vec(),
            },
        );
    }

    Ok(())
}
