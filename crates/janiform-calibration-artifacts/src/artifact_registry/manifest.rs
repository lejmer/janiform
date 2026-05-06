use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};

use super::super::checkpointing::CalibrationStageCheckpointMetadata;
use super::super::contracts::{
    CalibrationArtifactKind, CalibrationArtifactManifest, CalibrationArtifactManifestIndexMetadata,
    CalibrationArtifactManifestMetricStatus, CalibrationArtifactManifestMetricStatusCount,
    CalibrationArtifactManifestModelVersion, CalibrationArtifactManifestValidationMetricRole,
    CalibrationArtifactManifestValidationMetricSummary, CalibrationArtifactReference,
    CalibrationExecutionMode, CalibrationRunContract, CalibrationStage,
};
use super::dataset_snapshot::{
    CalibrationDatasetInputName, CalibrationDatasetSnapshotArtifact,
    calibration_dataset_snapshot_artifact_reference,
};
use super::shared::{
    CalibrationArtifactIoError, CalibrationArtifactPersistenceKind,
    calibration_committed_artifact_run_directory, calibration_stage_token,
    load_validated_json_artifact, persist_validated_json_artifact,
};
use super::stage_parameter::CalibrationStageParameterArtifact;
use super::validation_metric::{
    CalibrationValidationMetricArtifact, CalibrationValidationMetricArtifactRole,
    CalibrationValidationMetricStatus,
};

pub fn calibration_artifact_manifest_path(
    persistence_root_directory: &Path,
    run_id: &str,
) -> PathBuf {
    let run_directory =
        calibration_committed_artifact_run_directory(persistence_root_directory, run_id);

    calibration_artifact_manifest_path_in_run_directory(run_directory.as_path())
}

pub(crate) fn calibration_artifact_manifest_path_in_run_directory(run_directory: &Path) -> PathBuf {
    run_directory.join("artifact_manifest.json")
}

pub fn calibration_artifact_manifest_reference(run_id: &str) -> String {
    format!("artifact://manifest/{run_id}")
}

pub fn build_calibration_artifact_manifest(
    run_contract: &CalibrationRunContract,
    dataset_snapshot_artifact: &CalibrationDatasetSnapshotArtifact,
    stage_parameter_artifacts: &[CalibrationStageParameterArtifact],
    emitted_checkpoints: &[CalibrationStageCheckpointMetadata],
    validation_metric_artifacts: &[CalibrationValidationMetricArtifact],
    additional_artifact_references: &[CalibrationArtifactReference],
    committed_at_utc: DateTime<Utc>,
) -> Result<CalibrationArtifactManifest, CalibrationArtifactManifestBuildError> {
    if run_contract.run_spec.execution_mode != CalibrationExecutionMode::Disk {
        return Err(
            CalibrationArtifactManifestBuildError::UnsupportedExecutionMode {
                expected: CalibrationExecutionMode::Disk,
                found: run_contract.run_spec.execution_mode,
            },
        );
    }

    dataset_snapshot_artifact.validate().map_err(|source| {
        CalibrationArtifactManifestBuildError::InvalidDatasetSnapshotArtifact {
            error_message: source.to_string(),
        }
    })?;
    if dataset_snapshot_artifact.run_id != run_contract.run_spec.run_id {
        return Err(
            CalibrationArtifactManifestBuildError::DatasetSnapshotRunIdMismatch {
                expected: run_contract.run_spec.run_id.clone(),
                found: dataset_snapshot_artifact.run_id.clone(),
            },
        );
    }

    let mut artifact_references = vec![CalibrationArtifactReference {
        artifact_id: "dataset_snapshot".to_string(),
        artifact_kind: CalibrationArtifactKind::DatasetSnapshot,
        artifact_reference: calibration_dataset_snapshot_artifact_reference(
            run_contract.run_spec.run_id.as_str(),
        ),
        stage: None,
    }];

    let mut stage_artifacts_by_stage = BTreeMap::new();
    for artifact in stage_parameter_artifacts {
        artifact.validate().map_err(|source| {
            CalibrationArtifactManifestBuildError::InvalidStageOutputArtifact {
                stage: artifact.stage,
                error_message: source.to_string(),
            }
        })?;

        if artifact.run_id != run_contract.run_spec.run_id {
            return Err(
                CalibrationArtifactManifestBuildError::StageOutputRunIdMismatch {
                    stage: artifact.stage,
                    expected: run_contract.run_spec.run_id.clone(),
                    found: artifact.run_id.clone(),
                },
            );
        }

        if stage_artifacts_by_stage
            .insert(artifact.stage, artifact)
            .is_some()
        {
            return Err(
                CalibrationArtifactManifestBuildError::DuplicateStageOutputArtifact {
                    stage: artifact.stage,
                },
            );
        }
    }

    for stage in &run_contract.execution_stage_order {
        let Some(artifact) = stage_artifacts_by_stage.get(stage) else {
            return Err(
                CalibrationArtifactManifestBuildError::MissingStageOutputArtifact { stage: *stage },
            );
        };
        artifact_references.push(CalibrationArtifactReference {
            artifact_id: format!("{}_stage_output", calibration_stage_token(*stage)),
            artifact_kind: CalibrationArtifactKind::StageOutput,
            artifact_reference: artifact.artifact_reference.clone(),
            stage: Some(*stage),
        });
    }

    let mut seen_checkpoint_stages = BTreeSet::new();
    for checkpoint in emitted_checkpoints {
        if checkpoint.run_id != run_contract.run_spec.run_id {
            return Err(
                CalibrationArtifactManifestBuildError::CheckpointRunIdMismatch {
                    stage: checkpoint.completed_stage,
                    expected: run_contract.run_spec.run_id.clone(),
                    found: checkpoint.run_id.clone(),
                },
            );
        }

        if !seen_checkpoint_stages.insert(checkpoint.completed_stage) {
            return Err(
                CalibrationArtifactManifestBuildError::DuplicateCheckpointArtifact {
                    stage: checkpoint.completed_stage,
                },
            );
        }

        artifact_references.push(CalibrationArtifactReference {
            artifact_id: format!(
                "{}_checkpoint",
                calibration_stage_token(checkpoint.completed_stage)
            ),
            artifact_kind: CalibrationArtifactKind::StageCheckpoint,
            artifact_reference: checkpoint.checkpoint_reference.clone(),
            stage: Some(checkpoint.completed_stage),
        });
    }

    let mut seen_validation_roles = BTreeSet::new();
    for validation_metric_artifact in validation_metric_artifacts {
        validation_metric_artifact.validate().map_err(|source| {
            CalibrationArtifactManifestBuildError::InvalidValidationMetricArtifact {
                artifact_role: validation_metric_artifact.artifact_role,
                error_message: source.to_string(),
            }
        })?;
        if validation_metric_artifact.run_id != run_contract.run_spec.run_id {
            return Err(
                CalibrationArtifactManifestBuildError::ValidationMetricRunIdMismatch {
                    artifact_role: validation_metric_artifact.artifact_role,
                    expected: run_contract.run_spec.run_id.clone(),
                    found: validation_metric_artifact.run_id.clone(),
                },
            );
        }
        if !seen_validation_roles.insert(validation_metric_artifact.artifact_role) {
            return Err(
                CalibrationArtifactManifestBuildError::DuplicateValidationMetricArtifact {
                    artifact_role: validation_metric_artifact.artifact_role,
                },
            );
        }

        artifact_references.push(CalibrationArtifactReference {
            artifact_id: format!(
                "validation_metric_{}",
                validation_metric_artifact.artifact_role.artifact_token()
            ),
            artifact_kind: validation_metric_artifact.artifact_role.artifact_kind(),
            artifact_reference: validation_metric_artifact.artifact_reference.clone(),
            stage: None,
        });
    }

    artifact_references.extend_from_slice(additional_artifact_references);

    let index_metadata = build_manifest_index_metadata(
        run_contract,
        dataset_snapshot_artifact,
        stage_parameter_artifacts,
        validation_metric_artifacts,
        committed_at_utc,
    )?;

    CalibrationArtifactManifest::try_new(
        run_contract.run_spec.run_id.clone(),
        index_metadata,
        artifact_references,
    )
    .map_err(
        |source| CalibrationArtifactManifestBuildError::ManifestValidationFailed {
            error_message: source.to_string(),
        },
    )
}

pub fn persist_calibration_artifact_manifest_to_disk(
    persistence_root_directory: &Path,
    manifest: &CalibrationArtifactManifest,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::Manifest;
    let manifest_path =
        calibration_artifact_manifest_path(persistence_root_directory, manifest.run_id.as_str());

    persist_validated_json_artifact(
        artifact_kind,
        manifest_path.as_path(),
        manifest,
        |payload| payload.validate().map_err(|source| source.to_string()),
    )
}

pub fn persist_calibration_artifact_manifest_to_run_directory(
    run_directory: &Path,
    manifest: &CalibrationArtifactManifest,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::Manifest;
    let manifest_path = calibration_artifact_manifest_path_in_run_directory(run_directory);

    persist_validated_json_artifact(
        artifact_kind,
        manifest_path.as_path(),
        manifest,
        |payload| payload.validate().map_err(|source| source.to_string()),
    )
}

pub fn load_calibration_artifact_manifest_from_disk(
    manifest_path: &Path,
) -> Result<CalibrationArtifactManifest, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::Manifest;

    load_validated_json_artifact(
        artifact_kind,
        manifest_path,
        |payload: &CalibrationArtifactManifest| {
            payload.validate().map_err(|source| source.to_string())
        },
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationArtifactManifestBuildError {
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    InvalidDatasetSnapshotArtifact {
        error_message: String,
    },
    DatasetSnapshotRunIdMismatch {
        expected: String,
        found: String,
    },
    MissingStageOutputArtifact {
        stage: CalibrationStage,
    },
    DuplicateStageOutputArtifact {
        stage: CalibrationStage,
    },
    InvalidStageOutputArtifact {
        stage: CalibrationStage,
        error_message: String,
    },
    StageOutputRunIdMismatch {
        stage: CalibrationStage,
        expected: String,
        found: String,
    },
    DuplicateCheckpointArtifact {
        stage: CalibrationStage,
    },
    CheckpointRunIdMismatch {
        stage: CalibrationStage,
        expected: String,
        found: String,
    },
    DuplicateValidationMetricArtifact {
        artifact_role: CalibrationValidationMetricArtifactRole,
    },
    InvalidValidationMetricArtifact {
        artifact_role: CalibrationValidationMetricArtifactRole,
        error_message: String,
    },
    ValidationMetricRunIdMismatch {
        artifact_role: CalibrationValidationMetricArtifactRole,
        expected: String,
        found: String,
    },
    ManifestValidationFailed {
        error_message: String,
    },
}

impl Display for CalibrationArtifactManifestBuildError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedExecutionMode { expected, found } => write!(
                formatter,
                "artifact manifest requires execution_mode={expected:?}, found `{found:?}`"
            ),
            Self::InvalidDatasetSnapshotArtifact { error_message } => write!(
                formatter,
                "artifact manifest received invalid dataset snapshot artifact: {error_message}"
            ),
            Self::DatasetSnapshotRunIdMismatch { expected, found } => write!(
                formatter,
                "dataset snapshot artifact must use run_id `{expected}`, found `{found}`"
            ),
            Self::MissingStageOutputArtifact { stage } => write!(
                formatter,
                "artifact manifest is missing stage output artifact for `{stage:?}`"
            ),
            Self::DuplicateStageOutputArtifact { stage } => write!(
                formatter,
                "artifact manifest received duplicate stage output artifact for `{stage:?}`"
            ),
            Self::InvalidStageOutputArtifact {
                stage,
                error_message,
            } => write!(
                formatter,
                "artifact manifest received invalid stage output artifact for `{stage:?}`: {error_message}"
            ),
            Self::StageOutputRunIdMismatch {
                stage,
                expected,
                found,
            } => write!(
                formatter,
                "stage output artifact for `{stage:?}` must use run_id `{expected}`, found `{found}`"
            ),
            Self::DuplicateCheckpointArtifact { stage } => write!(
                formatter,
                "artifact manifest received duplicate checkpoint for `{stage:?}`"
            ),
            Self::CheckpointRunIdMismatch {
                stage,
                expected,
                found,
            } => write!(
                formatter,
                "checkpoint artifact for `{stage:?}` must use run_id `{expected}`, found `{found}`"
            ),
            Self::DuplicateValidationMetricArtifact { artifact_role } => write!(
                formatter,
                "artifact manifest received duplicate validation metric artifact for `{artifact_role:?}`"
            ),
            Self::InvalidValidationMetricArtifact {
                artifact_role,
                error_message,
            } => write!(
                formatter,
                "artifact manifest received invalid validation metric artifact for `{artifact_role:?}`: {error_message}"
            ),
            Self::ValidationMetricRunIdMismatch {
                artifact_role,
                expected,
                found,
            } => write!(
                formatter,
                "validation metric artifact for `{artifact_role:?}` must use run_id `{expected}`, found `{found}`"
            ),
            Self::ManifestValidationFailed { error_message } => {
                write!(
                    formatter,
                    "artifact manifest validation failed: {error_message}"
                )
            }
        }
    }
}

impl Error for CalibrationArtifactManifestBuildError {}

fn build_manifest_index_metadata(
    run_contract: &CalibrationRunContract,
    dataset_snapshot_artifact: &CalibrationDatasetSnapshotArtifact,
    stage_parameter_artifacts: &[CalibrationStageParameterArtifact],
    validation_metric_artifacts: &[CalibrationValidationMetricArtifact],
    committed_at_utc: DateTime<Utc>,
) -> Result<CalibrationArtifactManifestIndexMetadata, CalibrationArtifactManifestBuildError> {
    let trade_events_dataset = dataset_snapshot_artifact
        .input_datasets
        .iter()
        .find(|input_dataset| {
            input_dataset.input_name == CalibrationDatasetInputName::TradeEventsDataReference
        })
        .ok_or_else(
            || CalibrationArtifactManifestBuildError::InvalidDatasetSnapshotArtifact {
                error_message: "dataset snapshot is missing trade_events_data_reference"
                    .to_string(),
            },
        )?;

    let option_surface_dataset =
        dataset_snapshot_artifact
            .input_datasets
            .iter()
            .find(|input_dataset| {
                input_dataset.input_name == CalibrationDatasetInputName::OptionSurfaceDataReference
            });

    let mut stage_artifacts_by_stage = BTreeMap::new();
    for artifact in stage_parameter_artifacts {
        stage_artifacts_by_stage.insert(artifact.stage, artifact);
    }

    let seasonality_artifact = stage_artifacts_by_stage.get(&CalibrationStage::Seasonality);
    let seasonality_estimator_version =
        seasonality_artifact.and_then(|artifact| match &artifact.stage_parameters {
            super::stage_parameter::CalibrationStageParameterPayload::Seasonality(payload) => {
                Some(payload.estimator_version.clone())
            }
            _ => None,
        });

    let validation_metric_summaries = validation_metric_artifacts
        .iter()
        .map(build_validation_metric_summary)
        .collect::<Vec<_>>();

    let committed_at_unix_nanos = committed_at_utc.timestamp_nanos_opt().ok_or(
        CalibrationArtifactManifestBuildError::ManifestValidationFailed {
            error_message: "committed_at_utc could not be represented as unix nanoseconds"
                .to_string(),
        },
    )?;

    Ok(CalibrationArtifactManifestIndexMetadata {
        committed_at_utc: committed_at_utc.to_rfc3339_opts(SecondsFormat::Nanos, true),
        committed_at_unix_nanos,
        exchange_code: dataset_snapshot_artifact.exchange_code.clone(),
        dataset_snapshot_checksum: dataset_snapshot_artifact.snapshot_checksum.clone(),
        trade_events_data_reference: trade_events_dataset.data_reference.clone(),
        trade_events_reference_checksum: trade_events_dataset.reference_checksum.clone(),
        option_surface_data_reference: option_surface_dataset
            .map(|input_dataset| input_dataset.data_reference.clone()),
        option_surface_reference_checksum: option_surface_dataset
            .map(|input_dataset| input_dataset.reference_checksum.clone()),
        model_version: CalibrationArtifactManifestModelVersion {
            run_contract_schema_version: run_contract.schema_version.clone(),
            run_spec_schema_version: run_contract.run_spec_schema_version.clone(),
            seasonality_stage_output_schema_version: seasonality_artifact
                .map(|artifact| artifact.stage_output_schema_version.clone()),
            hawkes_stage_output_schema_version: stage_artifacts_by_stage
                .get(&CalibrationStage::Hawkes)
                .map(|artifact| artifact.stage_output_schema_version.clone()),
            return_components_stage_output_schema_version: stage_artifacts_by_stage
                .get(&CalibrationStage::ReturnComponents)
                .map(|artifact| artifact.stage_output_schema_version.clone()),
            mean_reversion_stage_output_schema_version: stage_artifacts_by_stage
                .get(&CalibrationStage::MeanReversion)
                .map(|artifact| artifact.stage_output_schema_version.clone()),
            joint_refinement_stage_output_schema_version: stage_artifacts_by_stage
                .get(&CalibrationStage::JointRefinement)
                .map(|artifact| artifact.stage_output_schema_version.clone()),
            seasonality_estimator_version,
        },
        validation_metric_summaries,
    })
}

fn build_validation_metric_summary(
    artifact: &CalibrationValidationMetricArtifact,
) -> CalibrationArtifactManifestValidationMetricSummary {
    let collection_status_counts = aggregate_status_counts(
        artifact
            .metric_collections
            .iter()
            .map(|collection| collection.status),
    );
    let metric_status_counts = aggregate_status_counts(
        artifact
            .metric_collections
            .iter()
            .flat_map(|collection| collection.metrics.iter().map(|metric| metric.status)),
    );

    let overall_status = derive_overall_validation_status(artifact);

    let mut failing_metric_ids = artifact
        .metric_collections
        .iter()
        .flat_map(|collection| {
            collection
                .metrics
                .iter()
                .filter(|metric| metric.status == CalibrationValidationMetricStatus::Fail)
                .map(|metric| metric.metric_id.clone())
        })
        .collect::<Vec<_>>();

    failing_metric_ids.sort();
    failing_metric_ids.dedup();

    CalibrationArtifactManifestValidationMetricSummary {
        artifact_role: manifest_validation_metric_role(artifact.artifact_role),
        artifact_reference: artifact.artifact_reference.clone(),
        overall_status: manifest_metric_status(overall_status),
        collection_count: artifact.metric_collections.len(),
        metric_count: artifact
            .metric_collections
            .iter()
            .map(|collection| collection.metrics.len())
            .sum(),
        collection_status_counts,
        metric_status_counts,
        failing_metric_ids,
    }
}

fn aggregate_status_counts<I>(statuses: I) -> Vec<CalibrationArtifactManifestMetricStatusCount>
where
    I: Iterator<Item = CalibrationValidationMetricStatus>,
{
    let mut counts = BTreeMap::new();
    for status in statuses {
        *counts
            .entry(manifest_metric_status(status))
            .or_insert(0usize) += 1;
    }

    counts
        .into_iter()
        .map(|(status, count)| CalibrationArtifactManifestMetricStatusCount { status, count })
        .collect()
}

fn derive_overall_validation_status(
    artifact: &CalibrationValidationMetricArtifact,
) -> CalibrationValidationMetricStatus {
    let mut statuses = artifact
        .metric_collections
        .iter()
        .map(|collection| collection.status)
        .collect::<Vec<_>>();
    statuses.extend(
        artifact
            .metric_collections
            .iter()
            .flat_map(|collection| collection.metrics.iter().map(|metric| metric.status)),
    );

    for status in [
        CalibrationValidationMetricStatus::Error,
        CalibrationValidationMetricStatus::Fail,
        CalibrationValidationMetricStatus::InsufficientData,
        CalibrationValidationMetricStatus::Missing,
        CalibrationValidationMetricStatus::Available,
        CalibrationValidationMetricStatus::Pass,
    ] {
        if statuses.contains(&status) {
            return status;
        }
    }

    CalibrationValidationMetricStatus::Pass
}

fn manifest_validation_metric_role(
    artifact_role: CalibrationValidationMetricArtifactRole,
) -> CalibrationArtifactManifestValidationMetricRole {
    match artifact_role {
        CalibrationValidationMetricArtifactRole::Diagnostics => {
            CalibrationArtifactManifestValidationMetricRole::Diagnostics
        }
        CalibrationValidationMetricArtifactRole::Comparison => {
            CalibrationArtifactManifestValidationMetricRole::Comparison
        }
    }
}

fn manifest_metric_status(
    status: CalibrationValidationMetricStatus,
) -> CalibrationArtifactManifestMetricStatus {
    match status {
        CalibrationValidationMetricStatus::Pass => CalibrationArtifactManifestMetricStatus::Pass,
        CalibrationValidationMetricStatus::Fail => CalibrationArtifactManifestMetricStatus::Fail,
        CalibrationValidationMetricStatus::InsufficientData => {
            CalibrationArtifactManifestMetricStatus::InsufficientData
        }
        CalibrationValidationMetricStatus::Available => {
            CalibrationArtifactManifestMetricStatus::Available
        }
        CalibrationValidationMetricStatus::Missing => {
            CalibrationArtifactManifestMetricStatus::Missing
        }
        CalibrationValidationMetricStatus::Error => CalibrationArtifactManifestMetricStatus::Error,
    }
}
