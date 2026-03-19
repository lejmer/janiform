use std::cmp::min;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::super::contracts::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CalibrationArtifactManifest,
    CalibrationArtifactManifestIndexMetadata, CalibrationArtifactManifestMetricStatus,
    CalibrationArtifactManifestValidationMetricRole, CalibrationExecutionMode,
};
use super::manifest::{
    calibration_artifact_manifest_path, load_calibration_artifact_manifest_from_disk,
};
use super::shared::{
    CalibrationArtifactIoError, CalibrationArtifactPersistenceKind,
    calibration_committed_artifact_runs_root_directory, calibration_experiment_index_path,
    load_validated_json_artifact, persist_validated_json_artifact,
};

pub const CALIBRATION_EXPERIMENT_INDEX_SCHEMA_VERSION: &str =
    "calibration.artifact_registry.experiment_index.v1";
pub const CALIBRATION_EXPERIMENT_INDEX_SCHEMA_FAMILY: &str =
    "calibration.artifact_registry.experiment_index";
pub const CALIBRATION_EXPERIMENT_INDEX_SCHEMA_GENERATION: &str = "v1";
pub const CALIBRATION_EXPERIMENT_INDEX_REBUILD_POLICY: &str =
    "rebuild_by_scanning_committed_run_manifests_and_rederiving_lookup_records.v1";
pub const CALIBRATION_EXPERIMENT_INDEX_ORDERING_POLICY: &str =
    "sort_by_committed_timestamp_desc_then_run_id_asc.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationExperimentIndexDocument {
    pub schema_version: String,
    pub schema_family: String,
    pub schema_generation: String,
    pub rebuild_policy: String,
    pub ordering_policy: String,
    pub records: Vec<CalibrationExperimentIndexRecord>,
}

impl CalibrationExperimentIndexDocument {
    pub fn empty() -> Self {
        Self {
            schema_version: CALIBRATION_EXPERIMENT_INDEX_SCHEMA_VERSION.to_string(),
            schema_family: CALIBRATION_EXPERIMENT_INDEX_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_EXPERIMENT_INDEX_SCHEMA_GENERATION.to_string(),
            rebuild_policy: CALIBRATION_EXPERIMENT_INDEX_REBUILD_POLICY.to_string(),
            ordering_policy: CALIBRATION_EXPERIMENT_INDEX_ORDERING_POLICY.to_string(),
            records: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), CalibrationExperimentIndexError> {
        if self.schema_version != CALIBRATION_EXPERIMENT_INDEX_SCHEMA_VERSION {
            return Err(CalibrationExperimentIndexError::InvalidSchemaMetadata {
                field: "schema_version",
                expected: CALIBRATION_EXPERIMENT_INDEX_SCHEMA_VERSION.to_string(),
                found: self.schema_version.clone(),
            });
        }
        if self.schema_family != CALIBRATION_EXPERIMENT_INDEX_SCHEMA_FAMILY {
            return Err(CalibrationExperimentIndexError::InvalidSchemaMetadata {
                field: "schema_family",
                expected: CALIBRATION_EXPERIMENT_INDEX_SCHEMA_FAMILY.to_string(),
                found: self.schema_family.clone(),
            });
        }
        if self.schema_generation != CALIBRATION_EXPERIMENT_INDEX_SCHEMA_GENERATION {
            return Err(CalibrationExperimentIndexError::InvalidSchemaMetadata {
                field: "schema_generation",
                expected: CALIBRATION_EXPERIMENT_INDEX_SCHEMA_GENERATION.to_string(),
                found: self.schema_generation.clone(),
            });
        }
        if self.rebuild_policy != CALIBRATION_EXPERIMENT_INDEX_REBUILD_POLICY {
            return Err(CalibrationExperimentIndexError::InvalidSchemaMetadata {
                field: "rebuild_policy",
                expected: CALIBRATION_EXPERIMENT_INDEX_REBUILD_POLICY.to_string(),
                found: self.rebuild_policy.clone(),
            });
        }
        if self.ordering_policy != CALIBRATION_EXPERIMENT_INDEX_ORDERING_POLICY {
            return Err(CalibrationExperimentIndexError::InvalidSchemaMetadata {
                field: "ordering_policy",
                expected: CALIBRATION_EXPERIMENT_INDEX_ORDERING_POLICY.to_string(),
                found: self.ordering_policy.clone(),
            });
        }

        let mut seen_run_ids = std::collections::BTreeSet::new();
        for record in &self.records {
            record.validate()?;
            if !seen_run_ids.insert(record.run_id.clone()) {
                return Err(CalibrationExperimentIndexError::DuplicateRunId {
                    run_id: record.run_id.clone(),
                });
            }
        }

        Ok(())
    }

    fn sort_records(&mut self) {
        self.records.sort_by(|left, right| {
            right
                .index_metadata
                .committed_at_unix_nanos
                .cmp(&left.index_metadata.committed_at_unix_nanos)
                .then_with(|| left.run_id.cmp(&right.run_id))
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationExperimentIndexRecord {
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub manifest_schema_version: String,
    pub index_metadata: CalibrationArtifactManifestIndexMetadata,
}

impl CalibrationExperimentIndexRecord {
    fn from_manifest(manifest: &CalibrationArtifactManifest) -> Self {
        Self {
            run_id: manifest.run_id.clone(),
            execution_mode: manifest.execution_mode,
            manifest_schema_version: manifest.schema_version.clone(),
            index_metadata: manifest.index_metadata.clone(),
        }
    }

    fn validate(&self) -> Result<(), CalibrationExperimentIndexError> {
        if self.run_id.trim().is_empty() {
            return Err(CalibrationExperimentIndexError::EmptyRunId);
        }
        if self.execution_mode != CalibrationExecutionMode::Disk {
            return Err(CalibrationExperimentIndexError::UnsupportedExecutionMode {
                expected: CalibrationExecutionMode::Disk,
                found: self.execution_mode,
            });
        }
        if self.manifest_schema_version != CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION {
            return Err(CalibrationExperimentIndexError::InvalidSchemaMetadata {
                field: "manifest_schema_version",
                expected: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
                found: self.manifest_schema_version.clone(),
            });
        }

        self.index_metadata.validate().map_err(|source| {
            CalibrationExperimentIndexError::InvalidIndexMetadata {
                run_id: self.run_id.clone(),
                error_message: source.to_string(),
            }
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CalibrationExperimentIndexLookupRequest {
    pub exchange_code: Option<String>,
    pub dataset_snapshot_checksum: Option<String>,
    pub trade_events_data_reference: Option<String>,
    pub trade_events_reference_checksum: Option<String>,
    pub option_surface_data_reference: Option<String>,
    pub option_surface_reference_checksum: Option<String>,
    pub run_contract_schema_version: Option<String>,
    pub run_spec_schema_version: Option<String>,
    pub seasonality_stage_output_schema_version: Option<String>,
    pub hawkes_stage_output_schema_version: Option<String>,
    pub return_components_stage_output_schema_version: Option<String>,
    pub joint_refinement_stage_output_schema_version: Option<String>,
    pub seasonality_estimator_version: Option<String>,
    pub diagnostics_overall_status: Option<CalibrationArtifactManifestMetricStatus>,
    pub comparison_overall_status: Option<CalibrationArtifactManifestMetricStatus>,
    pub failing_metric_id: Option<String>,
    pub committed_at_or_after_unix_nanos: Option<i64>,
    pub committed_before_unix_nanos: Option<i64>,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalibrationExperimentIndexLookupResponse {
    pub total_matching_records: usize,
    pub offset: usize,
    pub limit: usize,
    pub next_offset: Option<usize>,
    pub records: Vec<CalibrationExperimentIndexRecord>,
}

pub fn calibration_experiment_index_path_for_root(persistence_root_directory: &Path) -> PathBuf {
    calibration_experiment_index_path(persistence_root_directory)
}

pub fn persist_calibration_experiment_index_to_disk(
    persistence_root_directory: &Path,
    document: &CalibrationExperimentIndexDocument,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::ExperimentIndex;
    let artifact_path = calibration_experiment_index_path(persistence_root_directory);

    persist_validated_json_artifact(
        artifact_kind,
        artifact_path.as_path(),
        document,
        |payload| payload.validate().map_err(|source| source.to_string()),
    )
}

pub fn load_calibration_experiment_index_from_disk(
    persistence_root_directory: &Path,
) -> Result<CalibrationExperimentIndexDocument, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::ExperimentIndex;
    let artifact_path = calibration_experiment_index_path(persistence_root_directory);

    load_validated_json_artifact(
        artifact_kind,
        artifact_path.as_path(),
        |payload: &CalibrationExperimentIndexDocument| {
            payload.validate().map_err(|source| source.to_string())
        },
    )
}

pub fn update_calibration_experiment_index_for_run(
    persistence_root_directory: &Path,
    run_id: &str,
) -> Result<PathBuf, CalibrationExperimentIndexError> {
    let manifest_path = calibration_artifact_manifest_path(persistence_root_directory, run_id);
    let manifest = load_calibration_artifact_manifest_from_disk(manifest_path.as_path())
        .map_err(CalibrationExperimentIndexError::IndexIo)?;

    manifest.validate().map_err(|source| {
        CalibrationExperimentIndexError::InvalidIndexMetadata {
            run_id: run_id.to_string(),
            error_message: source.to_string(),
        }
    })?;

    let record = CalibrationExperimentIndexRecord::from_manifest(&manifest);
    record.validate()?;

    let mut document = load_existing_or_empty_index(persistence_root_directory)?;
    document
        .records
        .retain(|existing_record| existing_record.run_id != run_id);
    document.records.push(record);
    document.sort_records();
    document.validate()?;

    persist_calibration_experiment_index_to_disk(persistence_root_directory, &document)
        .map_err(CalibrationExperimentIndexError::IndexIo)
}

pub fn rebuild_calibration_experiment_index_from_disk(
    persistence_root_directory: &Path,
) -> Result<CalibrationExperimentIndexDocument, CalibrationExperimentIndexError> {
    let committed_runs_root_directory =
        calibration_committed_artifact_runs_root_directory(persistence_root_directory);
    let mut document = CalibrationExperimentIndexDocument::empty();

    if !committed_runs_root_directory.exists() {
        persist_calibration_experiment_index_to_disk(persistence_root_directory, &document)
            .map_err(CalibrationExperimentIndexError::IndexIo)?;
        return Ok(document);
    }

    let run_directories =
        fs::read_dir(committed_runs_root_directory.as_path()).map_err(|source| {
            CalibrationExperimentIndexError::RebuildScanFailed {
                path: committed_runs_root_directory.display().to_string(),
                error_message: source.to_string(),
            }
        })?;

    for directory_entry in run_directories {
        let directory_entry = directory_entry.map_err(|source| {
            CalibrationExperimentIndexError::RebuildScanFailed {
                path: committed_runs_root_directory.display().to_string(),
                error_message: source.to_string(),
            }
        })?;

        let file_type = directory_entry.file_type().map_err(|source| {
            CalibrationExperimentIndexError::RebuildScanFailed {
                path: directory_entry.path().display().to_string(),
                error_message: source.to_string(),
            }
        })?;
        if !file_type.is_dir() {
            continue;
        }

        let run_id = directory_entry.file_name().to_string_lossy().to_string();

        let manifest_path = calibration_artifact_manifest_path(persistence_root_directory, &run_id);
        let manifest = load_calibration_artifact_manifest_from_disk(manifest_path.as_path())
            .map_err(CalibrationExperimentIndexError::IndexIo)?;

        let record = CalibrationExperimentIndexRecord::from_manifest(&manifest);
        record.validate()?;

        document.records.push(record);
    }

    document.sort_records();
    document.validate()?;

    persist_calibration_experiment_index_to_disk(persistence_root_directory, &document)
        .map_err(CalibrationExperimentIndexError::IndexIo)?;

    Ok(document)
}

pub fn lookup_calibration_experiment_index_records(
    persistence_root_directory: &Path,
    request: &CalibrationExperimentIndexLookupRequest,
) -> Result<CalibrationExperimentIndexLookupResponse, CalibrationExperimentIndexError> {
    validate_lookup_request(request)?;

    let document = load_calibration_experiment_index_from_disk(persistence_root_directory)
        .map_err(CalibrationExperimentIndexError::IndexIo)?;
    let matching_records = document
        .records
        .into_iter()
        .filter(|record| record_matches_lookup_request(record, request))
        .collect::<Vec<_>>();

    let total_matching_records = matching_records.len();
    let end_offset = min(
        request.offset.saturating_add(request.limit),
        total_matching_records,
    );
    let records = if request.offset >= total_matching_records {
        Vec::new()
    } else {
        matching_records[request.offset..end_offset].to_vec()
    };
    let next_offset = (end_offset < total_matching_records).then_some(end_offset);

    Ok(CalibrationExperimentIndexLookupResponse {
        total_matching_records,
        offset: request.offset,
        limit: request.limit,
        next_offset,
        records,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationExperimentIndexError {
    IndexIo(CalibrationArtifactIoError),
    InvalidSchemaMetadata {
        field: &'static str,
        expected: String,
        found: String,
    },
    EmptyRunId,
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    DuplicateRunId {
        run_id: String,
    },
    InvalidIndexMetadata {
        run_id: String,
        error_message: String,
    },
    InvalidLookupRequest {
        error_message: String,
    },
    RebuildScanFailed {
        path: String,
        error_message: String,
    },
}

impl Display for CalibrationExperimentIndexError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IndexIo(source) => write!(formatter, "{source}"),
            Self::InvalidSchemaMetadata {
                field,
                expected,
                found,
            } => write!(formatter, "{field} must be `{expected}`, found `{found}`"),
            Self::EmptyRunId => write!(
                formatter,
                "experiment index records must use a non-empty run_id"
            ),
            Self::UnsupportedExecutionMode { expected, found } => write!(
                formatter,
                "experiment index records require execution_mode={expected:?}, found `{found:?}`"
            ),
            Self::DuplicateRunId { run_id } => write!(
                formatter,
                "experiment index contains duplicate run_id `{run_id}`"
            ),
            Self::InvalidIndexMetadata {
                run_id,
                error_message,
            } => write!(
                formatter,
                "experiment index metadata for run `{run_id}` is invalid: {error_message}"
            ),
            Self::InvalidLookupRequest { error_message } => write!(
                formatter,
                "experiment index lookup request is invalid: {error_message}"
            ),
            Self::RebuildScanFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to scan committed artifact directories in `{path}`: {error_message}"
            ),
        }
    }
}

impl Error for CalibrationExperimentIndexError {}

fn load_existing_or_empty_index(
    persistence_root_directory: &Path,
) -> Result<CalibrationExperimentIndexDocument, CalibrationExperimentIndexError> {
    let index_path = calibration_experiment_index_path(persistence_root_directory);
    if !index_path.exists() {
        return Ok(CalibrationExperimentIndexDocument::empty());
    }

    load_calibration_experiment_index_from_disk(persistence_root_directory)
        .map_err(CalibrationExperimentIndexError::IndexIo)
}

fn validate_lookup_request(
    request: &CalibrationExperimentIndexLookupRequest,
) -> Result<(), CalibrationExperimentIndexError> {
    if request.limit == 0 {
        return Err(CalibrationExperimentIndexError::InvalidLookupRequest {
            error_message: "limit must be positive".to_string(),
        });
    }
    if let (Some(start_nanos), Some(end_nanos)) = (
        request.committed_at_or_after_unix_nanos,
        request.committed_before_unix_nanos,
    ) {
        if start_nanos >= end_nanos {
            return Err(CalibrationExperimentIndexError::InvalidLookupRequest {
                error_message:
                    "committed_at_or_after_unix_nanos must be strictly less than committed_before_unix_nanos"
                        .to_string(),
            });
        }
    }

    Ok(())
}

fn record_matches_lookup_request(
    record: &CalibrationExperimentIndexRecord,
    request: &CalibrationExperimentIndexLookupRequest,
) -> bool {
    let metadata = &record.index_metadata;
    let model_version = &metadata.model_version;

    if let Some(exchange_code) = &request.exchange_code {
        if metadata.exchange_code != *exchange_code {
            return false;
        }
    }
    if let Some(dataset_snapshot_checksum) = &request.dataset_snapshot_checksum {
        if metadata.dataset_snapshot_checksum != *dataset_snapshot_checksum {
            return false;
        }
    }
    if let Some(trade_events_data_reference) = &request.trade_events_data_reference {
        if metadata.trade_events_data_reference != *trade_events_data_reference {
            return false;
        }
    }
    if let Some(trade_events_reference_checksum) = &request.trade_events_reference_checksum {
        if metadata.trade_events_reference_checksum != *trade_events_reference_checksum {
            return false;
        }
    }
    if let Some(option_surface_data_reference) = &request.option_surface_data_reference {
        if metadata.option_surface_data_reference.as_ref() != Some(option_surface_data_reference) {
            return false;
        }
    }
    if let Some(option_surface_reference_checksum) = &request.option_surface_reference_checksum {
        if metadata.option_surface_reference_checksum.as_ref()
            != Some(option_surface_reference_checksum)
        {
            return false;
        }
    }
    if let Some(run_contract_schema_version) = &request.run_contract_schema_version {
        if model_version.run_contract_schema_version != *run_contract_schema_version {
            return false;
        }
    }
    if let Some(run_spec_schema_version) = &request.run_spec_schema_version {
        if model_version.run_spec_schema_version != *run_spec_schema_version {
            return false;
        }
    }
    if let Some(stage_schema_version) = &request.seasonality_stage_output_schema_version {
        if model_version
            .seasonality_stage_output_schema_version
            .as_ref()
            != Some(stage_schema_version)
        {
            return false;
        }
    }
    if let Some(stage_schema_version) = &request.hawkes_stage_output_schema_version {
        if model_version.hawkes_stage_output_schema_version.as_ref() != Some(stage_schema_version) {
            return false;
        }
    }
    if let Some(stage_schema_version) = &request.return_components_stage_output_schema_version {
        if model_version
            .return_components_stage_output_schema_version
            .as_ref()
            != Some(stage_schema_version)
        {
            return false;
        }
    }
    if let Some(stage_schema_version) = &request.joint_refinement_stage_output_schema_version {
        if model_version
            .joint_refinement_stage_output_schema_version
            .as_ref()
            != Some(stage_schema_version)
        {
            return false;
        }
    }
    if let Some(seasonality_estimator_version) = &request.seasonality_estimator_version {
        if model_version.seasonality_estimator_version.as_ref()
            != Some(seasonality_estimator_version)
        {
            return false;
        }
    }
    if let Some(diagnostics_overall_status) = request.diagnostics_overall_status {
        if validation_summary_by_role(
            metadata,
            CalibrationArtifactManifestValidationMetricRole::Diagnostics,
        )
        .map(|summary| summary.overall_status)
            != Some(diagnostics_overall_status)
        {
            return false;
        }
    }
    if let Some(comparison_overall_status) = request.comparison_overall_status {
        if validation_summary_by_role(
            metadata,
            CalibrationArtifactManifestValidationMetricRole::Comparison,
        )
        .map(|summary| summary.overall_status)
            != Some(comparison_overall_status)
        {
            return false;
        }
    }
    if let Some(failing_metric_id) = &request.failing_metric_id {
        let has_failing_metric = metadata.validation_metric_summaries.iter().any(|summary| {
            summary
                .failing_metric_ids
                .iter()
                .any(|metric_id| metric_id == failing_metric_id)
        });
        if !has_failing_metric {
            return false;
        }
    }
    if let Some(start_nanos) = request.committed_at_or_after_unix_nanos {
        if metadata.committed_at_unix_nanos < start_nanos {
            return false;
        }
    }
    if let Some(end_nanos) = request.committed_before_unix_nanos {
        if metadata.committed_at_unix_nanos >= end_nanos {
            return false;
        }
    }

    true
}

fn validation_summary_by_role(
    metadata: &CalibrationArtifactManifestIndexMetadata,
    artifact_role: CalibrationArtifactManifestValidationMetricRole,
) -> Option<&super::super::contracts::CalibrationArtifactManifestValidationMetricSummary> {
    metadata
        .validation_metric_summaries
        .iter()
        .find(|summary| summary.artifact_role == artifact_role)
}
