use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contracts::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY,
    CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY, CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunContract, CalibrationStage,
    CalibrationStageSeedSource, resolve_calibration_stage_seed_metadata,
};
use super::stages::hawkes::HawkesStageFittedPayload;
use super::stages::return_components::ReturnComponentsStageFittedPayload;
use super::stages::seasonality::SeasonalityStageNormalizedPayload;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationStageCheckpointMetadata {
    pub schema_version: String,
    pub checkpoint_reference_policy: String,
    pub disk_write_policy: String,
    pub artifact_manifest_schema_version: String,
    pub checkpoint_reference: String,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub run_contract_schema_version: String,
    pub run_spec_schema_version: String,
    pub stage_graph_schema_version: String,
    pub execution_stage_order: Vec<CalibrationStage>,
    pub completed_stage: CalibrationStage,
    pub completed_stage_position: usize,
    pub run_global_seed: u64,
    pub completed_stage_output_schema_version: String,
    pub completed_stage_seed: u64,
    pub completed_stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: String,
    pub completed_stage_solver_settings: CalibrationOptimizerSettings,
}

impl CalibrationStageCheckpointMetadata {
    pub(crate) fn validate_compatibility_with_run_contract(
        &self,
        run_contract: &CalibrationRunContract,
    ) -> Result<(), CalibrationStageCheckpointCompatibilityError> {
        if self.schema_version != CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION {
            return Err(
                CalibrationStageCheckpointCompatibilityError::UnsupportedCheckpointSchemaVersion {
                    expected: CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION.to_string(),
                    found: self.schema_version.clone(),
                },
            );
        }
        if self.checkpoint_reference_policy != CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY {
            return Err(
                CalibrationStageCheckpointCompatibilityError::UnsupportedReferencePolicy {
                    expected: CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY.to_string(),
                    found: self.checkpoint_reference_policy.clone(),
                },
            );
        }
        if self.disk_write_policy != CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY {
            return Err(
                CalibrationStageCheckpointCompatibilityError::UnsupportedDiskWritePolicy {
                    expected: CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY.to_string(),
                    found: self.disk_write_policy.clone(),
                },
            );
        }
        if self.artifact_manifest_schema_version != CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION {
            return Err(
                CalibrationStageCheckpointCompatibilityError::UnsupportedArtifactManifestSchemaVersion {
                    expected: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
                    found: self.artifact_manifest_schema_version.clone(),
                },
            );
        }
        if self.run_id != run_contract.run_spec.run_id {
            return Err(
                CalibrationStageCheckpointCompatibilityError::RunIdMismatch {
                    expected: run_contract.run_spec.run_id.clone(),
                    found: self.run_id.clone(),
                },
            );
        }
        if self.execution_mode != run_contract.run_spec.execution_mode {
            return Err(
                CalibrationStageCheckpointCompatibilityError::ExecutionModeMismatch {
                    expected: run_contract.run_spec.execution_mode,
                    found: self.execution_mode,
                },
            );
        }
        if self.run_contract_schema_version != run_contract.schema_version {
            return Err(
                CalibrationStageCheckpointCompatibilityError::RunContractSchemaVersionMismatch {
                    expected: run_contract.schema_version.clone(),
                    found: self.run_contract_schema_version.clone(),
                },
            );
        }
        if self.run_spec_schema_version != run_contract.run_spec_schema_version {
            return Err(
                CalibrationStageCheckpointCompatibilityError::RunSpecSchemaVersionMismatch {
                    expected: run_contract.run_spec_schema_version.clone(),
                    found: self.run_spec_schema_version.clone(),
                },
            );
        }
        if self.stage_graph_schema_version != run_contract.stage_graph_schema_version {
            return Err(
                CalibrationStageCheckpointCompatibilityError::StageGraphSchemaVersionMismatch {
                    expected: run_contract.stage_graph_schema_version.clone(),
                    found: self.stage_graph_schema_version.clone(),
                },
            );
        }
        if self.execution_stage_order != run_contract.execution_stage_order {
            return Err(
                CalibrationStageCheckpointCompatibilityError::ExecutionStageOrderMismatch {
                    expected: run_contract.execution_stage_order.clone(),
                    found: self.execution_stage_order.clone(),
                },
            );
        }
        if self.run_global_seed != run_contract.run_spec.seed_settings.global_seed {
            return Err(
                CalibrationStageCheckpointCompatibilityError::RunGlobalSeedMismatch {
                    expected: run_contract.run_spec.seed_settings.global_seed,
                    found: self.run_global_seed,
                },
            );
        }
        if self.stage_seed_derivation_policy != CALIBRATION_STAGE_SEED_DERIVATION_POLICY {
            return Err(
                CalibrationStageCheckpointCompatibilityError::StageSeedDerivationPolicyMismatch {
                    expected: CALIBRATION_STAGE_SEED_DERIVATION_POLICY.to_string(),
                    found: self.stage_seed_derivation_policy.clone(),
                },
            );
        }

        let Some(expected_stage_position) = self
            .execution_stage_order
            .iter()
            .position(|stage| *stage == self.completed_stage)
        else {
            return Err(
                CalibrationStageCheckpointCompatibilityError::CompletedStageIsNotInExecutionOrder {
                    completed_stage: self.completed_stage,
                },
            );
        };

        if self.completed_stage_position != expected_stage_position {
            return Err(
                CalibrationStageCheckpointCompatibilityError::CompletedStagePositionMismatch {
                    completed_stage: self.completed_stage,
                    expected_position: expected_stage_position,
                    found_position: self.completed_stage_position,
                },
            );
        }

        let expected_stage_seed_metadata =
            resolve_calibration_stage_seed_metadata(&run_contract.run_spec, self.completed_stage);

        if self.completed_stage_seed != expected_stage_seed_metadata.stage_seed {
            return Err(
                CalibrationStageCheckpointCompatibilityError::CompletedStageSeedMismatch {
                    completed_stage: self.completed_stage,
                    expected_seed: expected_stage_seed_metadata.stage_seed,
                    found_seed: self.completed_stage_seed,
                },
            );
        }
        if self.completed_stage_seed_source != expected_stage_seed_metadata.stage_seed_source {
            return Err(
                CalibrationStageCheckpointCompatibilityError::CompletedStageSeedSourceMismatch {
                    completed_stage: self.completed_stage,
                    expected_source: expected_stage_seed_metadata.stage_seed_source,
                    found_source: self.completed_stage_seed_source,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationStageCheckpointCompatibilityError {
    UnsupportedCheckpointSchemaVersion {
        expected: String,
        found: String,
    },
    UnsupportedReferencePolicy {
        expected: String,
        found: String,
    },
    UnsupportedDiskWritePolicy {
        expected: String,
        found: String,
    },
    UnsupportedArtifactManifestSchemaVersion {
        expected: String,
        found: String,
    },
    RunIdMismatch {
        expected: String,
        found: String,
    },
    ExecutionModeMismatch {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    RunContractSchemaVersionMismatch {
        expected: String,
        found: String,
    },
    RunSpecSchemaVersionMismatch {
        expected: String,
        found: String,
    },
    StageGraphSchemaVersionMismatch {
        expected: String,
        found: String,
    },
    ExecutionStageOrderMismatch {
        expected: Vec<CalibrationStage>,
        found: Vec<CalibrationStage>,
    },
    RunGlobalSeedMismatch {
        expected: u64,
        found: u64,
    },
    StageSeedDerivationPolicyMismatch {
        expected: String,
        found: String,
    },
    CompletedStageIsNotInExecutionOrder {
        completed_stage: CalibrationStage,
    },
    CompletedStagePositionMismatch {
        completed_stage: CalibrationStage,
        expected_position: usize,
        found_position: usize,
    },
    CompletedStageSeedMismatch {
        completed_stage: CalibrationStage,
        expected_seed: u64,
        found_seed: u64,
    },
    CompletedStageSeedSourceMismatch {
        completed_stage: CalibrationStage,
        expected_source: CalibrationStageSeedSource,
        found_source: CalibrationStageSeedSource,
    },
}

impl Display for CalibrationStageCheckpointCompatibilityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedCheckpointSchemaVersion { expected, found } => write!(
                formatter,
                "checkpoint schema version is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::UnsupportedReferencePolicy { expected, found } => write!(
                formatter,
                "checkpoint reference policy is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::UnsupportedDiskWritePolicy { expected, found } => write!(
                formatter,
                "checkpoint disk write policy is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::UnsupportedArtifactManifestSchemaVersion { expected, found } => write!(
                formatter,
                "checkpoint artifact manifest schema is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::RunIdMismatch { expected, found } => write!(
                formatter,
                "checkpoint run_id is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::ExecutionModeMismatch { expected, found } => write!(
                formatter,
                "checkpoint execution_mode is incompatible, expected `{expected:?}`, found `{found:?}`"
            ),
            Self::RunContractSchemaVersionMismatch { expected, found } => write!(
                formatter,
                "checkpoint run_contract schema version is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::RunSpecSchemaVersionMismatch { expected, found } => write!(
                formatter,
                "checkpoint run_spec schema version is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::StageGraphSchemaVersionMismatch { expected, found } => write!(
                formatter,
                "checkpoint stage_graph schema version is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::ExecutionStageOrderMismatch { expected, found } => write!(
                formatter,
                "checkpoint execution order is incompatible, expected `{expected:?}`, found `{found:?}`"
            ),
            Self::RunGlobalSeedMismatch { expected, found } => write!(
                formatter,
                "checkpoint run_global_seed is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::StageSeedDerivationPolicyMismatch { expected, found } => write!(
                formatter,
                "checkpoint stage_seed_derivation_policy is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::CompletedStageIsNotInExecutionOrder { completed_stage } => write!(
                formatter,
                "checkpoint completed_stage `{completed_stage:?}` is not present in checkpoint execution order"
            ),
            Self::CompletedStagePositionMismatch {
                completed_stage,
                expected_position,
                found_position,
            } => write!(
                formatter,
                "checkpoint completed_stage `{completed_stage:?}` is at unexpected position, expected `{expected_position}`, found `{found_position}`"
            ),
            Self::CompletedStageSeedMismatch {
                completed_stage,
                expected_seed,
                found_seed,
            } => write!(
                formatter,
                "checkpoint completed_stage seed for `{completed_stage:?}` is incompatible, expected `{expected_seed}`, found `{found_seed}`"
            ),
            Self::CompletedStageSeedSourceMismatch {
                completed_stage,
                expected_source,
                found_source,
            } => write!(
                formatter,
                "checkpoint completed_stage seed source for `{completed_stage:?}` is incompatible, expected `{expected_source:?}`, found `{found_source:?}`"
            ),
        }
    }
}

impl Error for CalibrationStageCheckpointCompatibilityError {}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct CalibrationInMemoryStagePayloadState {
    pub(crate) seasonality_stage_payload: Option<SeasonalityStageNormalizedPayload>,
    pub(crate) hawkes_stage_payload: Option<HawkesStageFittedPayload>,
    pub(crate) return_components_stage_payload: Option<ReturnComponentsStageFittedPayload>,
}

#[derive(Debug, Clone, Default)]
struct CalibrationInMemoryCheckpointRunState {
    checkpoint_by_stage: BTreeMap<CalibrationStage, CalibrationStageCheckpointMetadata>,
    last_successful_stage: Option<CalibrationStage>,
    payload_state: CalibrationInMemoryStagePayloadState,
}

#[derive(Debug, Clone, Default)]
pub struct CalibrationMemoryCheckpointStore {
    run_state_by_id: BTreeMap<String, CalibrationInMemoryCheckpointRunState>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedInMemoryCheckpointState {
    pub(crate) checkpoint_metadata: CalibrationStageCheckpointMetadata,
    pub(crate) payload_state: CalibrationInMemoryStagePayloadState,
}

impl CalibrationMemoryCheckpointStore {
    pub const fn new() -> Self {
        Self {
            run_state_by_id: BTreeMap::new(),
        }
    }

    pub fn checkpoint_count_for_run(&self, run_id: &str) -> usize {
        self.run_state_by_id
            .get(run_id)
            .map(|state| state.checkpoint_by_stage.len())
            .unwrap_or(0)
    }

    pub fn checkpoint_for_stage(
        &self,
        run_id: &str,
        stage: CalibrationStage,
    ) -> Option<&CalibrationStageCheckpointMetadata> {
        self.run_state_by_id
            .get(run_id)
            .and_then(|state| state.checkpoint_by_stage.get(&stage))
    }

    pub fn last_checkpoint_for_run(
        &self,
        run_id: &str,
    ) -> Option<&CalibrationStageCheckpointMetadata> {
        let run_state = self.run_state_by_id.get(run_id)?;
        let last_stage = run_state.last_successful_stage?;

        run_state.checkpoint_by_stage.get(&last_stage)
    }

    pub(crate) fn load_resume_state(&self, run_id: &str) -> Option<LoadedInMemoryCheckpointState> {
        let run_state = self.run_state_by_id.get(run_id)?;
        let last_stage = run_state.last_successful_stage?;
        let checkpoint_metadata = run_state.checkpoint_by_stage.get(&last_stage)?.clone();

        Some(LoadedInMemoryCheckpointState {
            checkpoint_metadata,
            payload_state: run_state.payload_state.clone(),
        })
    }

    pub(crate) fn record_stage_checkpoint(
        &mut self,
        checkpoint_metadata: CalibrationStageCheckpointMetadata,
        payload_state: &CalibrationInMemoryStagePayloadState,
    ) {
        let run_state = self
            .run_state_by_id
            .entry(checkpoint_metadata.run_id.clone())
            .or_default();

        run_state.checkpoint_by_stage.insert(
            checkpoint_metadata.completed_stage,
            checkpoint_metadata.clone(),
        );
        run_state.last_successful_stage = Some(checkpoint_metadata.completed_stage);
        run_state.payload_state = payload_state.clone();
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum CalibrationCheckpointReferenceTarget {
    LastSuccess,
    Stage(CalibrationStage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedCalibrationCheckpointReference {
    run_id: String,
    target: CalibrationCheckpointReferenceTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CalibrationCheckpointReferenceParseError {
    MissingPrefix,
    MissingRunId,
    MissingTarget,
    UnknownTarget { token: String },
    UnexpectedPathSegments,
}

impl Display for CalibrationCheckpointReferenceParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPrefix => write!(
                formatter,
                "checkpoint reference must use `checkpoint://` prefix"
            ),
            Self::MissingRunId => write!(
                formatter,
                "checkpoint reference must include a non-empty run_id segment"
            ),
            Self::MissingTarget => write!(
                formatter,
                "checkpoint reference must include a stage token or `last-success`"
            ),
            Self::UnknownTarget { token } => write!(
                formatter,
                "checkpoint reference target `{token}` is unsupported"
            ),
            Self::UnexpectedPathSegments => write!(
                formatter,
                "checkpoint reference must have exactly two path segments after `checkpoint://`"
            ),
        }
    }
}

impl Error for CalibrationCheckpointReferenceParseError {}

fn parse_checkpoint_reference(
    checkpoint_reference: &str,
) -> Result<ParsedCalibrationCheckpointReference, CalibrationCheckpointReferenceParseError> {
    let Some(without_prefix) = checkpoint_reference.strip_prefix("checkpoint://") else {
        return Err(CalibrationCheckpointReferenceParseError::MissingPrefix);
    };

    let mut segments = without_prefix.split('/');
    let Some(run_id) = segments.next() else {
        return Err(CalibrationCheckpointReferenceParseError::MissingRunId);
    };
    if run_id.trim().is_empty() {
        return Err(CalibrationCheckpointReferenceParseError::MissingRunId);
    }

    let Some(target_segment) = segments.next() else {
        return Err(CalibrationCheckpointReferenceParseError::MissingTarget);
    };
    if target_segment.trim().is_empty() {
        return Err(CalibrationCheckpointReferenceParseError::MissingTarget);
    }
    if segments.next().is_some() {
        return Err(CalibrationCheckpointReferenceParseError::UnexpectedPathSegments);
    }

    let target = if target_segment == "last-success" {
        CalibrationCheckpointReferenceTarget::LastSuccess
    } else if let Some(stage) = calibration_stage_from_token(target_segment) {
        CalibrationCheckpointReferenceTarget::Stage(stage)
    } else {
        return Err(CalibrationCheckpointReferenceParseError::UnknownTarget {
            token: target_segment.to_string(),
        });
    };

    Ok(ParsedCalibrationCheckpointReference {
        run_id: run_id.to_string(),
        target,
    })
}

#[derive(Debug)]
pub(crate) enum CalibrationDiskCheckpointError {
    InvalidCheckpointReference {
        checkpoint_reference: String,
        source: CalibrationCheckpointReferenceParseError,
    },
    CreateDirectoryFailed {
        path: String,
        error_message: String,
    },
    ReadFailed {
        path: String,
        error_message: String,
    },
    WriteFailed {
        path: String,
        error_message: String,
    },
    RenameFailed {
        source_path: String,
        target_path: String,
        error_message: String,
    },
    SerializationFailed {
        error_message: String,
    },
    DeserializationFailed {
        path: String,
        error_message: String,
    },
}

impl Display for CalibrationDiskCheckpointError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCheckpointReference {
                checkpoint_reference,
                source,
            } => write!(
                formatter,
                "invalid checkpoint reference `{checkpoint_reference}`: {source}"
            ),
            Self::CreateDirectoryFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to create checkpoint directory `{path}`: {error_message}"
            ),
            Self::ReadFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to read checkpoint file `{path}`: {error_message}"
            ),
            Self::WriteFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to write checkpoint file `{path}`: {error_message}"
            ),
            Self::RenameFailed {
                source_path,
                target_path,
                error_message,
            } => write!(
                formatter,
                "failed to atomically rename checkpoint file `{source_path}` -> `{target_path}`: {error_message}"
            ),
            Self::SerializationFailed { error_message } => {
                write!(formatter, "failed to serialize checkpoint: {error_message}")
            }
            Self::DeserializationFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to deserialize checkpoint file `{path}`: {error_message}"
            ),
        }
    }
}

impl Error for CalibrationDiskCheckpointError {}

pub(crate) fn persist_stage_checkpoint_to_disk(
    disk_checkpoint_directory: &Path,
    checkpoint_metadata: &CalibrationStageCheckpointMetadata,
) -> Result<(), CalibrationDiskCheckpointError> {
    let run_directory = disk_checkpoint_directory.join(checkpoint_metadata.run_id.as_str());
    fs::create_dir_all(&run_directory).map_err(|source| {
        CalibrationDiskCheckpointError::CreateDirectoryFailed {
            path: run_directory.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    let stage_checkpoint_path = run_directory.join(format!(
        "{}.checkpoint.json",
        calibration_stage_token(checkpoint_metadata.completed_stage)
    ));
    write_checkpoint_file_atomically(&stage_checkpoint_path, checkpoint_metadata)?;

    let last_success_checkpoint_path = run_directory.join("last-success.checkpoint.json");
    write_checkpoint_file_atomically(&last_success_checkpoint_path, checkpoint_metadata)?;

    Ok(())
}

pub(crate) fn load_stage_checkpoint_from_disk(
    disk_checkpoint_directory: &Path,
    checkpoint_reference: &str,
) -> Result<CalibrationStageCheckpointMetadata, CalibrationDiskCheckpointError> {
    let parsed_reference = parse_checkpoint_reference(checkpoint_reference).map_err(|source| {
        CalibrationDiskCheckpointError::InvalidCheckpointReference {
            checkpoint_reference: checkpoint_reference.to_string(),
            source,
        }
    })?;

    let run_directory = disk_checkpoint_directory.join(parsed_reference.run_id.as_str());
    let checkpoint_path = match parsed_reference.target {
        CalibrationCheckpointReferenceTarget::LastSuccess => {
            run_directory.join("last-success.checkpoint.json")
        }
        CalibrationCheckpointReferenceTarget::Stage(stage) => run_directory.join(format!(
            "{}.checkpoint.json",
            calibration_stage_token(stage)
        )),
    };

    let content = fs::read_to_string(&checkpoint_path).map_err(|source| {
        CalibrationDiskCheckpointError::ReadFailed {
            path: checkpoint_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    serde_json::from_str(content.as_str()).map_err(|source| {
        CalibrationDiskCheckpointError::DeserializationFailed {
            path: checkpoint_path.display().to_string(),
            error_message: source.to_string(),
        }
    })
}

fn write_checkpoint_file_atomically(
    target_path: &Path,
    checkpoint_metadata: &CalibrationStageCheckpointMetadata,
) -> Result<(), CalibrationDiskCheckpointError> {
    let serialized = serde_json::to_vec_pretty(checkpoint_metadata).map_err(|source| {
        CalibrationDiskCheckpointError::SerializationFailed {
            error_message: source.to_string(),
        }
    })?;

    let temporary_path = target_path.with_extension("json.tmp");
    let mut temporary_file = File::create(&temporary_path).map_err(|source| {
        CalibrationDiskCheckpointError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;
    temporary_file
        .write_all(serialized.as_slice())
        .map_err(|source| CalibrationDiskCheckpointError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        })?;
    temporary_file
        .sync_all()
        .map_err(|source| CalibrationDiskCheckpointError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        })?;

    fs::rename(&temporary_path, target_path).map_err(|source| {
        CalibrationDiskCheckpointError::RenameFailed {
            source_path: temporary_path.display().to_string(),
            target_path: target_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    Ok(())
}

pub(crate) fn calibration_stage_token(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "seasonality",
        CalibrationStage::Hawkes => "hawkes",
        CalibrationStage::ReturnComponents => "return_components",
    }
}

pub(crate) fn calibration_stage_from_token(token: &str) -> Option<CalibrationStage> {
    match token {
        "seasonality" => Some(CalibrationStage::Seasonality),
        "hawkes" => Some(CalibrationStage::Hawkes),
        "return_components" => Some(CalibrationStage::ReturnComponents),
        _ => None,
    }
}
