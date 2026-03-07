use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::super::contracts::{
    CalibrationArtifactKind, CalibrationArtifactManifest, CalibrationStage,
};
use super::dataset_snapshot::{
    CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_VERSION, CalibrationDatasetSnapshotArtifact,
    calibration_dataset_snapshot_artifact_path, calibration_dataset_snapshot_artifact_reference,
};
use super::manifest::calibration_artifact_manifest_path;
use super::stage_parameter::{
    CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION, CalibrationStageParameterArtifact,
    calibration_stage_parameter_artifact_path,
};
use super::validation_metric::{
    CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_VERSION, CalibrationValidationMetricArtifact,
    CalibrationValidationMetricArtifactReferenceIntegrityError,
    CalibrationValidationMetricArtifactRole, calibration_validation_metric_artifact_path,
    calibration_validation_metric_artifact_reference,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationArtifactReloadSelection {
    CompleteRun,
    StageSubset(Vec<CalibrationStage>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalibrationArtifactReloadRequest {
    pub run_id: String,
    pub selection: CalibrationArtifactReloadSelection,
}

impl CalibrationArtifactReloadRequest {
    pub fn complete_run(
        run_id: impl Into<String>,
    ) -> Result<Self, CalibrationArtifactReloadRequestError> {
        let request = Self {
            run_id: run_id.into(),
            selection: CalibrationArtifactReloadSelection::CompleteRun,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn stage_subset(
        run_id: impl Into<String>,
        requested_stages: Vec<CalibrationStage>,
    ) -> Result<Self, CalibrationArtifactReloadRequestError> {
        let request = Self {
            run_id: run_id.into(),
            selection: CalibrationArtifactReloadSelection::StageSubset(requested_stages),
        };
        request.validate()?;
        Ok(request)
    }

    fn validate(&self) -> Result<(), CalibrationArtifactReloadRequestError> {
        if self.run_id.trim().is_empty() {
            return Err(CalibrationArtifactReloadRequestError::EmptyRunId);
        }

        if let CalibrationArtifactReloadSelection::StageSubset(requested_stages) = &self.selection {
            if requested_stages.is_empty() {
                return Err(CalibrationArtifactReloadRequestError::MissingRequestedStages);
            }

            let mut seen_stages = BTreeSet::new();
            for stage in requested_stages {
                if !seen_stages.insert(*stage) {
                    return Err(
                        CalibrationArtifactReloadRequestError::DuplicateRequestedStage {
                            stage: *stage,
                        },
                    );
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationArtifactReloadRequestError {
    EmptyRunId,
    MissingRequestedStages,
    DuplicateRequestedStage { stage: CalibrationStage },
}

impl Display for CalibrationArtifactReloadRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyRunId => write!(formatter, "run_id must be a non-empty string"),
            Self::MissingRequestedStages => write!(
                formatter,
                "stage_subset reload requests must include at least one stage"
            ),
            Self::DuplicateRequestedStage { stage } => write!(
                formatter,
                "stage_subset reload requests contain duplicate stage `{stage:?}`"
            ),
        }
    }
}

impl Error for CalibrationArtifactReloadRequestError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationArtifactReloadComponent {
    Manifest,
    DatasetSnapshot,
    StageParameter {
        stage: CalibrationStage,
    },
    ValidationMetric {
        artifact_role: CalibrationValidationMetricArtifactRole,
    },
}

impl Display for CalibrationArtifactReloadComponent {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manifest => write!(formatter, "artifact manifest"),
            Self::DatasetSnapshot => write!(formatter, "dataset snapshot artifact"),
            Self::StageParameter { stage } => {
                write!(formatter, "stage parameter artifact for `{stage:?}`")
            }
            Self::ValidationMetric { artifact_role } => write!(
                formatter,
                "validation metric artifact for role `{}`",
                artifact_role.artifact_token()
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationReloadedArtifactRun {
    pub manifest: CalibrationArtifactManifest,
    pub dataset_snapshot: CalibrationDatasetSnapshotArtifact,
    pub stage_parameter_artifacts: Vec<CalibrationStageParameterArtifact>,
    pub diagnostics_artifact: Option<CalibrationValidationMetricArtifact>,
    pub comparison_artifact: Option<CalibrationValidationMetricArtifact>,
}

impl CalibrationReloadedArtifactRun {
    pub fn stage_parameter_artifact(
        &self,
        stage: CalibrationStage,
    ) -> Option<&CalibrationStageParameterArtifact> {
        self.stage_parameter_artifacts
            .iter()
            .find(|artifact| artifact.stage == stage)
    }
}

pub fn reload_calibration_artifact_run_from_disk(
    persistence_root_directory: &Path,
    request: &CalibrationArtifactReloadRequest,
) -> Result<CalibrationReloadedArtifactRun, CalibrationArtifactReloadError> {
    request
        .validate()
        .map_err(CalibrationArtifactReloadError::InvalidRequest)?;

    let run_directory = super::shared::calibration_committed_artifact_run_directory(
        persistence_root_directory,
        request.run_id.as_str(),
    );
    if !run_directory.exists() {
        return Err(CalibrationArtifactReloadError::RunArtifactsUnavailable {
            run_id: request.run_id.clone(),
            path: run_directory.display().to_string(),
        });
    }

    let manifest_path =
        calibration_artifact_manifest_path(persistence_root_directory, request.run_id.as_str());
    let manifest = load_artifact_with_schema_check::<CalibrationArtifactManifest, _>(
        manifest_path.as_path(),
        CalibrationArtifactReloadComponent::Manifest,
        CalibrationArtifactManifest::schema_version(),
        |payload: &CalibrationArtifactManifest| {
            payload.validate().map_err(|source| source.to_string())
        },
    )?;
    ensure_run_id_matches(
        CalibrationArtifactReloadComponent::Manifest,
        request.run_id.as_str(),
        manifest.run_id.as_str(),
    )?;

    let dataset_reference = require_unique_manifest_reference(
        manifest_path.as_path(),
        &manifest,
        request.run_id.as_str(),
        CalibrationArtifactKind::DatasetSnapshot,
        None,
        CalibrationArtifactReloadComponent::DatasetSnapshot,
    )?;
    let expected_dataset_reference =
        calibration_dataset_snapshot_artifact_reference(request.run_id.as_str());
    ensure_reference_matches(
        CalibrationArtifactReloadComponent::DatasetSnapshot,
        expected_dataset_reference.as_str(),
        dataset_reference.as_str(),
    )?;

    let dataset_snapshot_path = calibration_dataset_snapshot_artifact_path(
        persistence_root_directory,
        request.run_id.as_str(),
    );
    let dataset_snapshot = load_artifact_with_schema_check::<CalibrationDatasetSnapshotArtifact, _>(
        dataset_snapshot_path.as_path(),
        CalibrationArtifactReloadComponent::DatasetSnapshot,
        CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_VERSION,
        |payload: &CalibrationDatasetSnapshotArtifact| {
            payload.validate().map_err(|source| source.to_string())
        },
    )?;
    ensure_run_id_matches(
        CalibrationArtifactReloadComponent::DatasetSnapshot,
        request.run_id.as_str(),
        dataset_snapshot.run_id.as_str(),
    )?;

    let available_stage_references = collect_stage_output_references(
        manifest_path.as_path(),
        &manifest,
        request.run_id.as_str(),
    )?;
    let requested_stages = resolve_requested_stages(request, &available_stage_references)?;

    let mut stage_parameter_artifacts = Vec::with_capacity(requested_stages.len());
    for stage in requested_stages {
        let expected_reference = available_stage_references.get(&stage).ok_or(
            CalibrationArtifactReloadError::MissingRequiredComponent {
                run_id: request.run_id.clone(),
                component: CalibrationArtifactReloadComponent::StageParameter { stage },
            },
        )?;

        let stage_path = calibration_stage_parameter_artifact_path(
            persistence_root_directory,
            request.run_id.as_str(),
            stage,
        );
        let stage_parameter_artifact =
            load_artifact_with_schema_check::<CalibrationStageParameterArtifact, _>(
                stage_path.as_path(),
                CalibrationArtifactReloadComponent::StageParameter { stage },
                CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION,
                |payload: &CalibrationStageParameterArtifact| {
                    payload.validate().map_err(|source| source.to_string())
                },
            )?;

        ensure_run_id_matches(
            CalibrationArtifactReloadComponent::StageParameter { stage },
            request.run_id.as_str(),
            stage_parameter_artifact.run_id.as_str(),
        )?;
        ensure_stage_matches(stage, stage_parameter_artifact.stage)?;
        ensure_reference_matches(
            CalibrationArtifactReloadComponent::StageParameter { stage },
            expected_reference.as_str(),
            stage_parameter_artifact.artifact_reference.as_str(),
        )?;

        stage_parameter_artifacts.push(stage_parameter_artifact);
    }

    stage_parameter_artifacts.sort_by_key(|artifact| artifact.stage);

    let (diagnostics_artifact, comparison_artifact) = if matches!(
        request.selection,
        CalibrationArtifactReloadSelection::CompleteRun
    ) {
        (
            load_optional_validation_metric_artifact(
                persistence_root_directory,
                request.run_id.as_str(),
                manifest_path.as_path(),
                &manifest,
                CalibrationValidationMetricArtifactRole::Diagnostics,
            )?,
            load_optional_validation_metric_artifact(
                persistence_root_directory,
                request.run_id.as_str(),
                manifest_path.as_path(),
                &manifest,
                CalibrationValidationMetricArtifactRole::Comparison,
            )?,
        )
    } else {
        (None, None)
    };

    Ok(CalibrationReloadedArtifactRun {
        manifest,
        dataset_snapshot,
        stage_parameter_artifacts,
        diagnostics_artifact,
        comparison_artifact,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationArtifactReloadError {
    InvalidRequest(CalibrationArtifactReloadRequestError),
    RunArtifactsUnavailable {
        run_id: String,
        path: String,
    },
    MissingArtifactFile {
        component: CalibrationArtifactReloadComponent,
        path: String,
    },
    ArtifactReadFailed {
        component: CalibrationArtifactReloadComponent,
        path: String,
        error_message: String,
    },
    UnsupportedSchema {
        component: CalibrationArtifactReloadComponent,
        path: String,
        expected_schema_version: String,
        found_schema_version: String,
    },
    CorruptedArtifact {
        component: CalibrationArtifactReloadComponent,
        path: String,
        error_message: String,
    },
    MissingRequiredComponent {
        run_id: String,
        component: CalibrationArtifactReloadComponent,
    },
    DuplicateManifestComponentReference {
        run_id: String,
        component: CalibrationArtifactReloadComponent,
        artifact_references: Vec<String>,
    },
    ArtifactRunIdMismatch {
        component: CalibrationArtifactReloadComponent,
        expected_run_id: String,
        found_run_id: String,
    },
    ArtifactReferenceMismatch {
        component: CalibrationArtifactReloadComponent,
        expected_reference: String,
        found_reference: String,
    },
    StageParameterStageMismatch {
        expected_stage: CalibrationStage,
        found_stage: CalibrationStage,
    },
    ValidationMetricIntegrityFailed {
        artifact_role: CalibrationValidationMetricArtifactRole,
        error_message: String,
    },
}

impl Display for CalibrationArtifactReloadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(source) => {
                write!(formatter, "reload request is invalid: {source}")
            }
            Self::RunArtifactsUnavailable { run_id, path } => write!(
                formatter,
                "no committed disk artifacts exist for run `{run_id}` in `{path}`"
            ),
            Self::MissingArtifactFile { component, path } => {
                write!(formatter, "missing {component} file at `{path}`")
            }
            Self::ArtifactReadFailed {
                component,
                path,
                error_message,
            } => write!(
                formatter,
                "failed to read {component} at `{path}`: {error_message}"
            ),
            Self::UnsupportedSchema {
                component,
                path,
                expected_schema_version,
                found_schema_version,
            } => write!(
                formatter,
                "{component} at `{path}` uses unsupported schema `{found_schema_version}`, expected `{expected_schema_version}`"
            ),
            Self::CorruptedArtifact {
                component,
                path,
                error_message,
            } => write!(
                formatter,
                "{component} at `{path}` is corrupted: {error_message}"
            ),
            Self::MissingRequiredComponent { run_id, component } => {
                write!(formatter, "run `{run_id}` is missing required {component}")
            }
            Self::DuplicateManifestComponentReference {
                run_id,
                component,
                artifact_references,
            } => write!(
                formatter,
                "run `{run_id}` records duplicate manifest references for {component}: {:?}",
                artifact_references
            ),
            Self::ArtifactRunIdMismatch {
                component,
                expected_run_id,
                found_run_id,
            } => write!(
                formatter,
                "{component} run_id must be `{expected_run_id}`, found `{found_run_id}`"
            ),
            Self::ArtifactReferenceMismatch {
                component,
                expected_reference,
                found_reference,
            } => write!(
                formatter,
                "{component} must use artifact_reference `{expected_reference}`, found `{found_reference}`"
            ),
            Self::StageParameterStageMismatch {
                expected_stage,
                found_stage,
            } => write!(
                formatter,
                "stage parameter artifact must represent stage `{expected_stage:?}`, found `{found_stage:?}`"
            ),
            Self::ValidationMetricIntegrityFailed {
                artifact_role,
                error_message,
            } => write!(
                formatter,
                "validation metric artifact for role `{}` failed manifest integrity checks: {error_message}",
                artifact_role.artifact_token()
            ),
        }
    }
}

impl Error for CalibrationArtifactReloadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRequest(source) => Some(source),
            _ => None,
        }
    }
}

fn resolve_requested_stages(
    request: &CalibrationArtifactReloadRequest,
    available_stage_references: &BTreeMap<CalibrationStage, String>,
) -> Result<Vec<CalibrationStage>, CalibrationArtifactReloadError> {
    match &request.selection {
        CalibrationArtifactReloadSelection::CompleteRun => {
            if available_stage_references.is_empty() {
                return Err(CalibrationArtifactReloadError::MissingRequiredComponent {
                    run_id: request.run_id.clone(),
                    component: CalibrationArtifactReloadComponent::StageParameter {
                        stage: CalibrationStage::Seasonality,
                    },
                });
            }

            Ok(available_stage_references.keys().copied().collect())
        }
        CalibrationArtifactReloadSelection::StageSubset(requested_stages) => {
            let mut sorted_requested_stages = requested_stages.clone();
            sorted_requested_stages.sort();

            for stage in &sorted_requested_stages {
                if !available_stage_references.contains_key(stage) {
                    return Err(CalibrationArtifactReloadError::MissingRequiredComponent {
                        run_id: request.run_id.clone(),
                        component: CalibrationArtifactReloadComponent::StageParameter {
                            stage: *stage,
                        },
                    });
                }
            }

            Ok(sorted_requested_stages)
        }
    }
}

fn load_optional_validation_metric_artifact(
    persistence_root_directory: &Path,
    run_id: &str,
    manifest_path: &Path,
    manifest: &CalibrationArtifactManifest,
    artifact_role: CalibrationValidationMetricArtifactRole,
) -> Result<Option<CalibrationValidationMetricArtifact>, CalibrationArtifactReloadError> {
    let component = CalibrationArtifactReloadComponent::ValidationMetric { artifact_role };

    let Some(expected_reference) = maybe_unique_manifest_reference(
        manifest_path,
        manifest,
        run_id,
        artifact_role.artifact_kind(),
        None,
        component,
    )?
    else {
        return Ok(None);
    };

    let validation_metric_path = calibration_validation_metric_artifact_path(
        persistence_root_directory,
        run_id,
        artifact_role,
    );
    let artifact = load_artifact_with_schema_check::<CalibrationValidationMetricArtifact, _>(
        validation_metric_path.as_path(),
        component,
        CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_VERSION,
        |payload: &CalibrationValidationMetricArtifact| {
            payload.validate().map_err(|source| source.to_string())
        },
    )?;

    ensure_run_id_matches(component, run_id, artifact.run_id.as_str())?;
    ensure_reference_matches(
        component,
        expected_reference.as_str(),
        artifact.artifact_reference.as_str(),
    )?;
    ensure_reference_matches(
        component,
        calibration_validation_metric_artifact_reference(run_id, artifact_role).as_str(),
        artifact.artifact_reference.as_str(),
    )?;

    artifact.validate_against_manifest(manifest).map_err(
        |source: CalibrationValidationMetricArtifactReferenceIntegrityError| {
            CalibrationArtifactReloadError::ValidationMetricIntegrityFailed {
                artifact_role,
                error_message: source.to_string(),
            }
        },
    )?;

    Ok(Some(artifact))
}

fn collect_stage_output_references(
    manifest_path: &Path,
    manifest: &CalibrationArtifactManifest,
    run_id: &str,
) -> Result<BTreeMap<CalibrationStage, String>, CalibrationArtifactReloadError> {
    let mut stage_references = BTreeMap::new();
    for stage in [
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
        CalibrationStage::ReturnComponents,
    ] {
        if let Some(artifact_reference) = maybe_unique_manifest_reference(
            manifest_path,
            manifest,
            run_id,
            CalibrationArtifactKind::StageOutput,
            Some(stage),
            CalibrationArtifactReloadComponent::StageParameter { stage },
        )? {
            stage_references.insert(stage, artifact_reference);
        }
    }

    Ok(stage_references)
}

fn require_unique_manifest_reference(
    manifest_path: &Path,
    manifest: &CalibrationArtifactManifest,
    run_id: &str,
    artifact_kind: CalibrationArtifactKind,
    stage: Option<CalibrationStage>,
    component: CalibrationArtifactReloadComponent,
) -> Result<String, CalibrationArtifactReloadError> {
    maybe_unique_manifest_reference(
        manifest_path,
        manifest,
        run_id,
        artifact_kind,
        stage,
        component,
    )?
    .ok_or(CalibrationArtifactReloadError::MissingRequiredComponent {
        run_id: run_id.to_string(),
        component,
    })
}

fn maybe_unique_manifest_reference(
    manifest_path: &Path,
    manifest: &CalibrationArtifactManifest,
    run_id: &str,
    artifact_kind: CalibrationArtifactKind,
    stage: Option<CalibrationStage>,
    component: CalibrationArtifactReloadComponent,
) -> Result<Option<String>, CalibrationArtifactReloadError> {
    let matching_references = manifest
        .artifact_references
        .iter()
        .filter(|artifact_reference| artifact_reference.artifact_kind == artifact_kind)
        .filter(|artifact_reference| artifact_reference.stage == stage)
        .map(|artifact_reference| artifact_reference.artifact_reference.clone())
        .collect::<Vec<_>>();

    match matching_references.as_slice() {
        [] => Ok(None),
        [artifact_reference] => Ok(Some(artifact_reference.clone())),
        _ => Err(
            CalibrationArtifactReloadError::DuplicateManifestComponentReference {
                run_id: run_id.to_string(),
                component,
                artifact_references: matching_references,
            },
        ),
    }
    .and_then(|artifact_reference| {
        if artifact_kind == CalibrationArtifactKind::StageOutput && stage.is_some() {
            let missing_stage_entry =
                manifest
                    .artifact_references
                    .iter()
                    .find(|artifact_reference| {
                        artifact_reference.artifact_kind == CalibrationArtifactKind::StageOutput
                            && artifact_reference.stage.is_none()
                    });
            if missing_stage_entry.is_some() {
                return Err(CalibrationArtifactReloadError::CorruptedArtifact {
                    component: CalibrationArtifactReloadComponent::Manifest,
                    path: manifest_path.display().to_string(),
                    error_message: "stage_output manifest entries must include a stage".to_string(),
                });
            }
        }

        Ok(artifact_reference)
    })
}

fn ensure_run_id_matches(
    component: CalibrationArtifactReloadComponent,
    expected_run_id: &str,
    found_run_id: &str,
) -> Result<(), CalibrationArtifactReloadError> {
    if found_run_id != expected_run_id {
        return Err(CalibrationArtifactReloadError::ArtifactRunIdMismatch {
            component,
            expected_run_id: expected_run_id.to_string(),
            found_run_id: found_run_id.to_string(),
        });
    }

    Ok(())
}

fn ensure_reference_matches(
    component: CalibrationArtifactReloadComponent,
    expected_reference: &str,
    found_reference: &str,
) -> Result<(), CalibrationArtifactReloadError> {
    if expected_reference != found_reference {
        return Err(CalibrationArtifactReloadError::ArtifactReferenceMismatch {
            component,
            expected_reference: expected_reference.to_string(),
            found_reference: found_reference.to_string(),
        });
    }

    Ok(())
}

fn ensure_stage_matches(
    expected_stage: CalibrationStage,
    found_stage: CalibrationStage,
) -> Result<(), CalibrationArtifactReloadError> {
    if found_stage != expected_stage {
        return Err(
            CalibrationArtifactReloadError::StageParameterStageMismatch {
                expected_stage,
                found_stage,
            },
        );
    }

    Ok(())
}

fn load_artifact_with_schema_check<T, Validate>(
    artifact_path: &Path,
    component: CalibrationArtifactReloadComponent,
    expected_schema_version: &str,
    validate: Validate,
) -> Result<T, CalibrationArtifactReloadError>
where
    T: DeserializeOwned,
    Validate: FnOnce(&T) -> Result<(), String>,
{
    let content = fs::read_to_string(artifact_path).map_err(|source| match source.kind() {
        std::io::ErrorKind::NotFound => CalibrationArtifactReloadError::MissingArtifactFile {
            component,
            path: artifact_path.display().to_string(),
        },
        _ => CalibrationArtifactReloadError::ArtifactReadFailed {
            component,
            path: artifact_path.display().to_string(),
            error_message: source.to_string(),
        },
    })?;

    let document: Value = serde_json::from_str(content.as_str()).map_err(|source| {
        CalibrationArtifactReloadError::CorruptedArtifact {
            component,
            path: artifact_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    let found_schema_version = extract_schema_version(artifact_path, component, &document)?;
    if found_schema_version != expected_schema_version {
        return Err(CalibrationArtifactReloadError::UnsupportedSchema {
            component,
            path: artifact_path.display().to_string(),
            expected_schema_version: expected_schema_version.to_string(),
            found_schema_version,
        });
    }

    let payload: T = serde_json::from_value(document).map_err(|source| {
        CalibrationArtifactReloadError::CorruptedArtifact {
            component,
            path: artifact_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;
    validate(&payload).map_err(|error_message| {
        CalibrationArtifactReloadError::CorruptedArtifact {
            component,
            path: artifact_path.display().to_string(),
            error_message,
        }
    })?;

    Ok(payload)
}

fn extract_schema_version(
    artifact_path: &Path,
    component: CalibrationArtifactReloadComponent,
    document: &Value,
) -> Result<String, CalibrationArtifactReloadError> {
    let document_map =
        document
            .as_object()
            .ok_or(CalibrationArtifactReloadError::CorruptedArtifact {
                component,
                path: artifact_path.display().to_string(),
                error_message: "expected a top-level JSON object".to_string(),
            })?;
    let schema_version = document_map
        .get("schema_version")
        .and_then(Value::as_str)
        .ok_or(CalibrationArtifactReloadError::CorruptedArtifact {
            component,
            path: artifact_path.display().to_string(),
            error_message: "missing or invalid top-level schema_version".to_string(),
        })?;

    Ok(schema_version.to_string())
}
