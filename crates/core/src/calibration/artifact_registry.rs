use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::benchmarking::fnv1a_64;

use super::checkpointing::CalibrationStageCheckpointMetadata;
use super::contracts::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY,
    CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CALIBRATION_RUN_DATA_INPUTS_SCHEMA_VERSION,
    CALIBRATION_RUN_SPEC_SCHEMA_VERSION, CalibrationArtifactKind, CalibrationArtifactManifest,
    CalibrationArtifactReference, CalibrationExecutionMode, CalibrationRunContract,
    CalibrationStage, CalibrationStageSeedSource,
};
use super::objective::CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION;
use super::staged_runner::CalibrationStagedRunStageReport;
use super::stages::hawkes::{HawkesStageExecutionReport, HawkesStageFittedParameters};
use super::stages::return_components::{
    ReturnComponentsStageExecutionReport, ReturnComponentsStageFittedParameters,
};
use super::stages::seasonality::{
    SeasonalityStageExecutionReport, SeasonalityStageNormalizationMetadata,
};

pub const CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_VERSION: &str =
    "calibration.artifact_registry.dataset_snapshot.v1";
pub const CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_FAMILY: &str =
    "calibration.artifact_registry.dataset_snapshot";
pub const CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_GENERATION: &str = "v1";
pub const CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY: &str =
    "breaking_changes_allowed_with_lockstep_in_repo_updates_pre_v0_1.v1";
pub const CALIBRATION_DATASET_SNAPSHOT_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "calibration.artifact_registry.dataset_snapshot.output_schema_metadata.v1";
pub const CALIBRATION_DATASET_SNAPSHOT_PERSISTENCE_SCOPE_POLICY: &str =
    "artifact_required_only_for_execution_mode_disk_when_persistence_is_enabled.v1";
pub const CALIBRATION_DATASET_SNAPSHOT_STAGE_INPUT_NAMING_POLICY: &str = "trade_events_data_reference_maps_to_seasonality_and_option_surface_data_reference_maps_to_return_components.v1";
pub const CALIBRATION_DATASET_SNAPSHOT_CHECKSUM_ALGORITHM: &str = "fnv1a_64_hex_utf8.v1";
pub const CALIBRATION_DATASET_REFERENCE_CHECKSUM_SCOPE: &str = "reference_string_utf8_bytes.v1";
pub const CALIBRATION_DATASET_SNAPSHOT_CHECKSUM_SCOPE: &str =
    "canonical_artifact_identity_reference_and_provenance_fields_utf8_bytes.v1";

const CALIBRATION_DATASET_SNAPSHOT_REQUIRED_ARTIFACT_FIELDS: [&str; 9] = [
    "schema_version",
    "run_id",
    "execution_mode",
    "schema_metadata",
    "output_schema_metadata",
    "exchange_code",
    "provenance",
    "input_datasets",
    "snapshot_checksum",
];
const CALIBRATION_DATASET_SNAPSHOT_REQUIRED_PROVENANCE_FIELDS: [&str; 9] = [
    "artifact_manifest_schema_version",
    "run_contract_schema_version",
    "run_spec_schema_version",
    "data_inputs_schema_version",
    "execution_mode_default_policy",
    "persistence_scope_policy",
    "stage_input_naming_policy",
    "reference_checksum_algorithm",
    "snapshot_checksum_algorithm",
];
const CALIBRATION_DATASET_SNAPSHOT_REQUIRED_INPUT_DATASET_FIELDS: [&str; 4] = [
    "input_name",
    "primary_stage",
    "data_reference",
    "reference_checksum",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationDatasetSnapshotSchemaMetadata {
    pub schema_family: String,
    pub schema_generation: String,
    pub forward_extension_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationDatasetSnapshotOutputSchemaMetadata {
    pub schema_version: String,
    pub required_artifact_fields: Vec<String>,
    pub required_provenance_fields: Vec<String>,
    pub required_input_dataset_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationDatasetSnapshotProvenance {
    pub artifact_manifest_schema_version: String,
    pub run_contract_schema_version: String,
    pub run_spec_schema_version: String,
    pub data_inputs_schema_version: String,
    pub execution_mode_default_policy: String,
    pub persistence_scope_policy: String,
    pub stage_input_naming_policy: String,
    pub reference_checksum_algorithm: String,
    pub snapshot_checksum_algorithm: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationDatasetInputName {
    TradeEventsDataReference,
    OptionSurfaceDataReference,
}

impl CalibrationDatasetInputName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TradeEventsDataReference => "trade_events_data_reference",
            Self::OptionSurfaceDataReference => "option_surface_data_reference",
        }
    }

    pub const fn primary_stage(self) -> CalibrationStage {
        match self {
            Self::TradeEventsDataReference => CalibrationStage::Seasonality,
            Self::OptionSurfaceDataReference => CalibrationStage::ReturnComponents,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationDatasetInputSnapshot {
    pub input_name: CalibrationDatasetInputName,
    pub primary_stage: CalibrationStage,
    pub data_reference: String,
    pub reference_checksum: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationDatasetSnapshotArtifact {
    pub schema_version: String,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub schema_metadata: CalibrationDatasetSnapshotSchemaMetadata,
    pub output_schema_metadata: CalibrationDatasetSnapshotOutputSchemaMetadata,
    pub exchange_code: String,
    pub provenance: CalibrationDatasetSnapshotProvenance,
    pub input_datasets: Vec<CalibrationDatasetInputSnapshot>,
    pub snapshot_checksum: String,
}

impl CalibrationDatasetSnapshotArtifact {
    pub fn validate(&self) -> Result<(), CalibrationDatasetSnapshotArtifactValidationError> {
        validate_exact_metadata_field(
            "schema_version",
            CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_VERSION,
            self.schema_version.as_str(),
        )?;

        if self.run_id.trim().is_empty() {
            return Err(CalibrationDatasetSnapshotArtifactValidationError::EmptyRunId);
        }
        if self.execution_mode != CalibrationExecutionMode::Disk {
            return Err(
                CalibrationDatasetSnapshotArtifactValidationError::UnsupportedExecutionMode {
                    expected: CalibrationExecutionMode::Disk,
                    found: self.execution_mode,
                },
            );
        }
        if self.exchange_code.trim().is_empty() {
            return Err(CalibrationDatasetSnapshotArtifactValidationError::EmptyExchangeCode);
        }

        validate_exact_metadata_field(
            "schema_metadata.schema_family",
            CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_FAMILY,
            self.schema_metadata.schema_family.as_str(),
        )?;
        validate_exact_metadata_field(
            "schema_metadata.schema_generation",
            CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_GENERATION,
            self.schema_metadata.schema_generation.as_str(),
        )?;
        validate_exact_metadata_field(
            "schema_metadata.forward_extension_policy",
            CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY,
            self.schema_metadata.forward_extension_policy.as_str(),
        )?;
        validate_exact_metadata_field(
            "output_schema_metadata.schema_version",
            CALIBRATION_DATASET_SNAPSHOT_OUTPUT_SCHEMA_METADATA_VERSION,
            self.output_schema_metadata.schema_version.as_str(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.required_artifact_fields",
            &CALIBRATION_DATASET_SNAPSHOT_REQUIRED_ARTIFACT_FIELDS,
            self.output_schema_metadata
                .required_artifact_fields
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.required_provenance_fields",
            &CALIBRATION_DATASET_SNAPSHOT_REQUIRED_PROVENANCE_FIELDS,
            self.output_schema_metadata
                .required_provenance_fields
                .as_slice(),
        )?;
        validate_exact_string_list(
            "output_schema_metadata.required_input_dataset_fields",
            &CALIBRATION_DATASET_SNAPSHOT_REQUIRED_INPUT_DATASET_FIELDS,
            self.output_schema_metadata
                .required_input_dataset_fields
                .as_slice(),
        )?;

        validate_non_empty_provenance_field(
            "provenance.artifact_manifest_schema_version",
            self.provenance.artifact_manifest_schema_version.as_str(),
        )?;
        validate_non_empty_provenance_field(
            "provenance.run_contract_schema_version",
            self.provenance.run_contract_schema_version.as_str(),
        )?;
        validate_non_empty_provenance_field(
            "provenance.run_spec_schema_version",
            self.provenance.run_spec_schema_version.as_str(),
        )?;
        validate_non_empty_provenance_field(
            "provenance.data_inputs_schema_version",
            self.provenance.data_inputs_schema_version.as_str(),
        )?;
        validate_exact_metadata_field(
            "provenance.artifact_manifest_schema_version",
            CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION,
            self.provenance.artifact_manifest_schema_version.as_str(),
        )?;
        validate_exact_metadata_field(
            "provenance.run_contract_schema_version",
            CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION,
            self.provenance.run_contract_schema_version.as_str(),
        )?;
        validate_exact_metadata_field(
            "provenance.run_spec_schema_version",
            CALIBRATION_RUN_SPEC_SCHEMA_VERSION,
            self.provenance.run_spec_schema_version.as_str(),
        )?;
        validate_exact_metadata_field(
            "provenance.data_inputs_schema_version",
            CALIBRATION_RUN_DATA_INPUTS_SCHEMA_VERSION,
            self.provenance.data_inputs_schema_version.as_str(),
        )?;
        validate_exact_metadata_field(
            "provenance.execution_mode_default_policy",
            CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY,
            self.provenance.execution_mode_default_policy.as_str(),
        )?;
        validate_exact_metadata_field(
            "provenance.persistence_scope_policy",
            CALIBRATION_DATASET_SNAPSHOT_PERSISTENCE_SCOPE_POLICY,
            self.provenance.persistence_scope_policy.as_str(),
        )?;
        validate_exact_metadata_field(
            "provenance.stage_input_naming_policy",
            CALIBRATION_DATASET_SNAPSHOT_STAGE_INPUT_NAMING_POLICY,
            self.provenance.stage_input_naming_policy.as_str(),
        )?;
        validate_exact_metadata_field(
            "provenance.reference_checksum_algorithm",
            CALIBRATION_DATASET_SNAPSHOT_CHECKSUM_ALGORITHM,
            self.provenance.reference_checksum_algorithm.as_str(),
        )?;
        validate_exact_metadata_field(
            "provenance.snapshot_checksum_algorithm",
            CALIBRATION_DATASET_SNAPSHOT_CHECKSUM_ALGORITHM,
            self.provenance.snapshot_checksum_algorithm.as_str(),
        )?;

        if self.input_datasets.is_empty() {
            return Err(CalibrationDatasetSnapshotArtifactValidationError::MissingInputDatasets);
        }

        let mut seen_input_names = BTreeSet::new();
        let mut has_trade_events_reference = false;
        for input_dataset in &self.input_datasets {
            if !seen_input_names.insert(input_dataset.input_name) {
                return Err(
                    CalibrationDatasetSnapshotArtifactValidationError::DuplicateInputDataset {
                        input_name: input_dataset.input_name,
                    },
                );
            }

            if input_dataset.data_reference.trim().is_empty() {
                return Err(
                    CalibrationDatasetSnapshotArtifactValidationError::EmptyInputDataReference {
                        input_name: input_dataset.input_name,
                    },
                );
            }

            let expected_primary_stage = input_dataset.input_name.primary_stage();
            if input_dataset.primary_stage != expected_primary_stage {
                return Err(
                    CalibrationDatasetSnapshotArtifactValidationError::InputDatasetStageMismatch {
                        input_name: input_dataset.input_name,
                        expected: expected_primary_stage,
                        found: input_dataset.primary_stage,
                    },
                );
            }

            validate_checksum_hex(
                input_dataset.reference_checksum.as_str(),
                CalibrationDatasetSnapshotArtifactValidationError::InvalidReferenceChecksumFormat {
                    input_name: input_dataset.input_name,
                    found: input_dataset.reference_checksum.clone(),
                },
            )?;

            let expected_reference_checksum =
                compute_reference_checksum_hex(input_dataset.data_reference.as_str());
            if input_dataset.reference_checksum != expected_reference_checksum {
                return Err(
                    CalibrationDatasetSnapshotArtifactValidationError::ReferenceChecksumMismatch {
                        input_name: input_dataset.input_name,
                        expected: expected_reference_checksum,
                        found: input_dataset.reference_checksum.clone(),
                    },
                );
            }

            if input_dataset.input_name == CalibrationDatasetInputName::TradeEventsDataReference {
                has_trade_events_reference = true;
            }
        }

        if !has_trade_events_reference {
            return Err(
                CalibrationDatasetSnapshotArtifactValidationError::MissingRequiredInputDataset {
                    input_name: CalibrationDatasetInputName::TradeEventsDataReference,
                },
            );
        }

        validate_checksum_hex(
            self.snapshot_checksum.as_str(),
            CalibrationDatasetSnapshotArtifactValidationError::InvalidSnapshotChecksumFormat {
                found: self.snapshot_checksum.clone(),
            },
        )?;

        let expected_snapshot_checksum = self.compute_snapshot_checksum_hex();
        if self.snapshot_checksum != expected_snapshot_checksum {
            return Err(
                CalibrationDatasetSnapshotArtifactValidationError::SnapshotChecksumMismatch {
                    expected: expected_snapshot_checksum,
                    found: self.snapshot_checksum.clone(),
                },
            );
        }

        Ok(())
    }

    pub fn compute_snapshot_checksum_hex(&self) -> String {
        let payload = self.snapshot_checksum_payload();
        format!("{:016x}", fnv1a_64(payload.as_bytes()))
    }

    fn snapshot_checksum_payload(&self) -> String {
        let mut segments = vec![
            format!("schema_version={}", self.schema_version),
            format!("run_id={}", self.run_id),
            format!(
                "execution_mode={}",
                calibration_execution_mode_token(self.execution_mode)
            ),
            format!("exchange_code={}", self.exchange_code),
            format!("schema_family={}", self.schema_metadata.schema_family),
            format!(
                "schema_generation={}",
                self.schema_metadata.schema_generation
            ),
            format!(
                "forward_extension_policy={}",
                self.schema_metadata.forward_extension_policy
            ),
            format!(
                "artifact_manifest_schema_version={}",
                self.provenance.artifact_manifest_schema_version
            ),
            format!(
                "run_contract_schema_version={}",
                self.provenance.run_contract_schema_version
            ),
            format!(
                "run_spec_schema_version={}",
                self.provenance.run_spec_schema_version
            ),
            format!(
                "data_inputs_schema_version={}",
                self.provenance.data_inputs_schema_version
            ),
            format!(
                "execution_mode_default_policy={}",
                self.provenance.execution_mode_default_policy
            ),
            format!(
                "persistence_scope_policy={}",
                self.provenance.persistence_scope_policy
            ),
            format!(
                "stage_input_naming_policy={}",
                self.provenance.stage_input_naming_policy
            ),
            format!(
                "reference_checksum_algorithm={}",
                self.provenance.reference_checksum_algorithm
            ),
            format!(
                "snapshot_checksum_algorithm={}",
                self.provenance.snapshot_checksum_algorithm
            ),
        ];

        let mut input_datasets = self.input_datasets.clone();
        input_datasets.sort_by_key(|input_dataset| input_dataset.input_name);
        for input_dataset in input_datasets {
            segments.push(format!("input_name={}", input_dataset.input_name.as_str()));
            segments.push(format!(
                "primary_stage={}",
                calibration_stage_token(input_dataset.primary_stage)
            ));
            segments.push(format!("data_reference={}", input_dataset.data_reference));
            segments.push(format!(
                "reference_checksum={}",
                input_dataset.reference_checksum
            ));
        }

        segments.join("\n")
    }
}

pub fn build_calibration_dataset_snapshot_artifact(
    run_contract: &CalibrationRunContract,
) -> Result<CalibrationDatasetSnapshotArtifact, CalibrationDatasetSnapshotArtifactValidationError> {
    let mut input_datasets = vec![CalibrationDatasetInputSnapshot {
        input_name: CalibrationDatasetInputName::TradeEventsDataReference,
        primary_stage: CalibrationStage::Seasonality,
        data_reference: run_contract
            .run_spec
            .data_inputs
            .trade_events_data_reference
            .clone(),
        reference_checksum: compute_reference_checksum_hex(
            run_contract
                .run_spec
                .data_inputs
                .trade_events_data_reference
                .as_str(),
        ),
    }];

    if let Some(option_surface_data_reference) = &run_contract
        .run_spec
        .data_inputs
        .option_surface_data_reference
    {
        input_datasets.push(CalibrationDatasetInputSnapshot {
            input_name: CalibrationDatasetInputName::OptionSurfaceDataReference,
            primary_stage: CalibrationStage::ReturnComponents,
            data_reference: option_surface_data_reference.clone(),
            reference_checksum: compute_reference_checksum_hex(option_surface_data_reference),
        });
    }

    let mut artifact = CalibrationDatasetSnapshotArtifact {
        schema_version: CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: run_contract.run_spec.run_id.clone(),
        execution_mode: run_contract.run_spec.execution_mode,
        schema_metadata: CalibrationDatasetSnapshotSchemaMetadata {
            schema_family: CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        output_schema_metadata: CalibrationDatasetSnapshotOutputSchemaMetadata {
            schema_version: CALIBRATION_DATASET_SNAPSHOT_OUTPUT_SCHEMA_METADATA_VERSION.to_string(),
            required_artifact_fields: CALIBRATION_DATASET_SNAPSHOT_REQUIRED_ARTIFACT_FIELDS
                .iter()
                .map(|field| (*field).to_string())
                .collect(),
            required_provenance_fields: CALIBRATION_DATASET_SNAPSHOT_REQUIRED_PROVENANCE_FIELDS
                .iter()
                .map(|field| (*field).to_string())
                .collect(),
            required_input_dataset_fields:
                CALIBRATION_DATASET_SNAPSHOT_REQUIRED_INPUT_DATASET_FIELDS
                    .iter()
                    .map(|field| (*field).to_string())
                    .collect(),
        },
        exchange_code: run_contract.run_spec.data_inputs.exchange_code.clone(),
        provenance: CalibrationDatasetSnapshotProvenance {
            artifact_manifest_schema_version: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION
                .to_string(),
            run_contract_schema_version: CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION.to_string(),
            run_spec_schema_version: CALIBRATION_RUN_SPEC_SCHEMA_VERSION.to_string(),
            data_inputs_schema_version: CALIBRATION_RUN_DATA_INPUTS_SCHEMA_VERSION.to_string(),
            execution_mode_default_policy: CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY.to_string(),
            persistence_scope_policy: CALIBRATION_DATASET_SNAPSHOT_PERSISTENCE_SCOPE_POLICY
                .to_string(),
            stage_input_naming_policy: CALIBRATION_DATASET_SNAPSHOT_STAGE_INPUT_NAMING_POLICY
                .to_string(),
            reference_checksum_algorithm: CALIBRATION_DATASET_SNAPSHOT_CHECKSUM_ALGORITHM
                .to_string(),
            snapshot_checksum_algorithm: CALIBRATION_DATASET_SNAPSHOT_CHECKSUM_ALGORITHM
                .to_string(),
        },
        input_datasets,
        snapshot_checksum: String::new(),
    };

    artifact.snapshot_checksum = artifact.compute_snapshot_checksum_hex();
    artifact.validate()?;

    Ok(artifact)
}

pub fn calibration_dataset_snapshot_artifact_reference(run_id: &str) -> String {
    format!("artifact://dataset_snapshot/{run_id}")
}

pub fn calibration_dataset_snapshot_artifact_path(
    disk_checkpoint_directory: &Path,
    run_id: &str,
) -> PathBuf {
    disk_checkpoint_directory
        .join(run_id)
        .join("dataset_snapshot.artifact.json")
}

pub fn persist_calibration_dataset_snapshot_artifact_to_disk(
    disk_checkpoint_directory: &Path,
    artifact: &CalibrationDatasetSnapshotArtifact,
) -> Result<PathBuf, CalibrationDatasetSnapshotArtifactIoError> {
    artifact.validate().map_err(|source| {
        CalibrationDatasetSnapshotArtifactIoError::ValidationFailed {
            error_message: source.to_string(),
        }
    })?;

    let artifact_path = calibration_dataset_snapshot_artifact_path(
        disk_checkpoint_directory,
        artifact.run_id.as_str(),
    );
    let run_directory = artifact_path
        .parent()
        .expect("dataset snapshot artifact path should always have a parent directory");
    fs::create_dir_all(run_directory).map_err(|source| {
        CalibrationDatasetSnapshotArtifactIoError::CreateDirectoryFailed {
            path: run_directory.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    write_json_file_atomically(&artifact_path, artifact)?;
    Ok(artifact_path)
}

pub fn persist_calibration_dataset_snapshot_from_run_contract_to_disk(
    disk_checkpoint_directory: &Path,
    run_contract: &CalibrationRunContract,
) -> Result<PathBuf, CalibrationDatasetSnapshotArtifactIoError> {
    let artifact = build_calibration_dataset_snapshot_artifact(run_contract).map_err(|source| {
        CalibrationDatasetSnapshotArtifactIoError::ValidationFailed {
            error_message: source.to_string(),
        }
    })?;
    persist_calibration_dataset_snapshot_artifact_to_disk(disk_checkpoint_directory, &artifact)
}

pub fn load_calibration_dataset_snapshot_artifact_from_disk(
    artifact_path: &Path,
) -> Result<CalibrationDatasetSnapshotArtifact, CalibrationDatasetSnapshotArtifactIoError> {
    let content = fs::read_to_string(artifact_path).map_err(|source| {
        CalibrationDatasetSnapshotArtifactIoError::ReadFailed {
            path: artifact_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    let artifact: CalibrationDatasetSnapshotArtifact = serde_json::from_str(content.as_str())
        .map_err(
            |source| CalibrationDatasetSnapshotArtifactIoError::DeserializationFailed {
                path: artifact_path.display().to_string(),
                error_message: source.to_string(),
            },
        )?;
    artifact.validate().map_err(|source| {
        CalibrationDatasetSnapshotArtifactIoError::ValidationFailed {
            error_message: source.to_string(),
        }
    })?;

    Ok(artifact)
}

pub const CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION: &str =
    "calibration.artifact_registry.stage_parameter.v1";
pub const CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY: &str =
    "calibration.artifact_registry.stage_parameter";
pub const CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION: &str = "v1";
pub const CALIBRATION_STAGE_PARAMETER_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "calibration.artifact_registry.stage_parameter.output_schema_metadata.v1";
pub const CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION: &str =
    "calibration.artifact_registry.stage_parameter.optimizer_state.v1";

const CALIBRATION_STAGE_PARAMETER_REQUIRED_ARTIFACT_FIELDS: [&str; 11] = [
    "schema_version",
    "run_id",
    "execution_mode",
    "stage",
    "artifact_reference",
    "schema_metadata",
    "output_schema_metadata",
    "stage_output_schema_version",
    "seed_metadata",
    "solver_settings",
    "stage_parameters",
];
const CALIBRATION_STAGE_PARAMETER_REQUIRED_SEED_METADATA_FIELDS: [&str; 4] = [
    "run_global_seed",
    "stage_seed",
    "stage_seed_source",
    "stage_seed_derivation_policy",
];
const CALIBRATION_STAGE_PARAMETER_REQUIRED_OPTIMIZER_STATE_FIELDS: [&str; 6] = [
    "schema_version",
    "optimizer_adapter_metadata",
    "input_candidate_count",
    "valid_candidate_count",
    "selected_candidate_raw_objective",
    "selected_candidate_penalized_objective",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStageParameterSchemaMetadata {
    pub schema_family: String,
    pub schema_generation: String,
    pub forward_extension_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStageParameterOutputSchemaMetadata {
    pub schema_version: String,
    pub required_artifact_fields: Vec<String>,
    pub required_seed_metadata_fields: Vec<String>,
    pub required_optimizer_state_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStageParameterSeedMetadata {
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStageParameterOptimizerState {
    pub schema_version: String,
    pub optimizer_adapter_metadata: CalibrationStageParameterOptimizerAdapterMetadata,
    pub input_candidate_count: usize,
    pub valid_candidate_count: usize,
    pub selected_candidate_raw_objective: f64,
    pub selected_candidate_penalized_objective: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStageParameterOptimizerAdapterMetadata {
    pub schema_version: String,
    pub backend_id: String,
    pub random_seed: u64,
    pub selection_policy: String,
    pub penalty_policy: String,
    pub bounds_policy: String,
    pub solver_settings: super::contracts::CalibrationOptimizerSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeasonalityStageParameterPayload {
    pub dataset_id: String,
    pub exchange_code: String,
    pub trade_events_data_reference: String,
    pub estimator_version: String,
    pub timestamp_event_input_schema_version: String,
    pub timestamp_event_preprocessed_schema_version: String,
    pub intraday_estimate_schema_version: String,
    pub day_of_week_estimate_schema_version: String,
    pub day_of_year_estimate_schema_version: String,
    pub composition_schema_version: String,
    pub intraday_minutes_per_bin: u16,
    pub intraday_factor_by_bin: Vec<f64>,
    pub day_of_week_factor: Vec<f64>,
    pub day_of_year_factor: Vec<f64>,
    pub normalization_metadata: SeasonalityStageNormalizationMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesStageParameterPayload {
    pub selected_candidate_id: String,
    pub fitted_parameters: HawkesStageFittedParameters,
    pub seasonality_input_transport: super::contracts::CalibrationStageHandoffTransport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seasonality_dataset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seasonality_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnComponentsStageParameterPayload {
    pub selected_candidate_id: String,
    pub fitted_parameters: ReturnComponentsStageFittedParameters,
    pub candidate_generation_policy: String,
    pub objective_weighting_policy: String,
    pub objective_metric_bucket_policy: String,
    pub activity_input_transport: super::contracts::CalibrationStageHandoffTransport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_selected_hawkes_candidate_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "payload_kind", content = "payload", rename_all = "snake_case")]
pub enum CalibrationStageParameterPayload {
    Seasonality(SeasonalityStageParameterPayload),
    Hawkes(HawkesStageParameterPayload),
    ReturnComponents(ReturnComponentsStageParameterPayload),
}

impl CalibrationStageParameterPayload {
    const fn stage(&self) -> CalibrationStage {
        match self {
            Self::Seasonality(_) => CalibrationStage::Seasonality,
            Self::Hawkes(_) => CalibrationStage::Hawkes,
            Self::ReturnComponents(_) => CalibrationStage::ReturnComponents,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStageParameterArtifact {
    pub schema_version: String,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub stage: CalibrationStage,
    pub artifact_reference: String,
    pub schema_metadata: CalibrationStageParameterSchemaMetadata,
    pub output_schema_metadata: CalibrationStageParameterOutputSchemaMetadata,
    pub stage_output_schema_version: String,
    pub seed_metadata: CalibrationStageParameterSeedMetadata,
    pub solver_settings: super::contracts::CalibrationOptimizerSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optimizer_state: Option<CalibrationStageParameterOptimizerState>,
    pub stage_parameters: CalibrationStageParameterPayload,
}

impl CalibrationStageParameterArtifact {
    pub fn validate(&self) -> Result<(), CalibrationStageParameterArtifactValidationError> {
        validate_stage_parameter_metadata_field(
            "schema_version",
            CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION,
            self.schema_version.as_str(),
        )?;

        if self.run_id.trim().is_empty() {
            return Err(CalibrationStageParameterArtifactValidationError::EmptyRunId);
        }
        if self.execution_mode != CalibrationExecutionMode::Disk {
            return Err(
                CalibrationStageParameterArtifactValidationError::UnsupportedExecutionMode {
                    expected: CalibrationExecutionMode::Disk,
                    found: self.execution_mode,
                },
            );
        }
        if self.artifact_reference.trim().is_empty() {
            return Err(
                CalibrationStageParameterArtifactValidationError::EmptyArtifactReference {
                    stage: self.stage,
                },
            );
        }
        if self.stage_output_schema_version.trim().is_empty() {
            return Err(
                CalibrationStageParameterArtifactValidationError::EmptyStageOutputSchemaVersion {
                    stage: self.stage,
                },
            );
        }

        validate_stage_parameter_metadata_field(
            "schema_metadata.schema_family",
            CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY,
            self.schema_metadata.schema_family.as_str(),
        )?;
        validate_stage_parameter_metadata_field(
            "schema_metadata.schema_generation",
            CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION,
            self.schema_metadata.schema_generation.as_str(),
        )?;
        validate_stage_parameter_metadata_field(
            "schema_metadata.forward_extension_policy",
            CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY,
            self.schema_metadata.forward_extension_policy.as_str(),
        )?;
        validate_stage_parameter_metadata_field(
            "output_schema_metadata.schema_version",
            CALIBRATION_STAGE_PARAMETER_OUTPUT_SCHEMA_METADATA_VERSION,
            self.output_schema_metadata.schema_version.as_str(),
        )?;
        validate_stage_parameter_required_string_list(
            "output_schema_metadata.required_artifact_fields",
            &CALIBRATION_STAGE_PARAMETER_REQUIRED_ARTIFACT_FIELDS,
            self.output_schema_metadata
                .required_artifact_fields
                .as_slice(),
        )?;
        validate_stage_parameter_required_string_list(
            "output_schema_metadata.required_seed_metadata_fields",
            &CALIBRATION_STAGE_PARAMETER_REQUIRED_SEED_METADATA_FIELDS,
            self.output_schema_metadata
                .required_seed_metadata_fields
                .as_slice(),
        )?;
        validate_stage_parameter_required_string_list(
            "output_schema_metadata.required_optimizer_state_fields",
            &CALIBRATION_STAGE_PARAMETER_REQUIRED_OPTIMIZER_STATE_FIELDS,
            self.output_schema_metadata
                .required_optimizer_state_fields
                .as_slice(),
        )?;

        if self
            .seed_metadata
            .stage_seed_derivation_policy
            .trim()
            .is_empty()
        {
            return Err(
                CalibrationStageParameterArtifactValidationError::EmptySeedMetadataField {
                    field: "seed_metadata.stage_seed_derivation_policy",
                },
            );
        }

        self.solver_settings.validate().map_err(|source| {
            CalibrationStageParameterArtifactValidationError::InvalidSolverSettings {
                stage: self.stage,
                error_message: source.to_string(),
            }
        })?;

        if self.stage_parameters.stage() != self.stage {
            return Err(
                CalibrationStageParameterArtifactValidationError::StagePayloadMismatch {
                    stage: self.stage,
                    payload_stage: self.stage_parameters.stage(),
                },
            );
        }

        match &self.stage_parameters {
            CalibrationStageParameterPayload::Seasonality(payload) => {
                validate_non_empty_artifact_value(
                    payload.dataset_id.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.seasonality.dataset_id",
                    },
                )?;
                validate_non_empty_artifact_value(
                    payload.exchange_code.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.seasonality.exchange_code",
                    },
                )?;
                validate_non_empty_artifact_value(
                    payload.trade_events_data_reference.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.seasonality.trade_events_data_reference",
                    },
                )?;
                validate_non_empty_artifact_value(
                    payload.estimator_version.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.seasonality.estimator_version",
                    },
                )?;
                if payload.intraday_factor_by_bin.is_empty()
                    || payload.day_of_week_factor.is_empty()
                    || payload.day_of_year_factor.is_empty()
                {
                    return Err(
                        CalibrationStageParameterArtifactValidationError::MissingSeasonalityFactors,
                    );
                }
                if self.optimizer_state.is_some() {
                    return Err(
                        CalibrationStageParameterArtifactValidationError::UnexpectedOptimizerState {
                            stage: self.stage,
                        },
                    );
                }
            }
            CalibrationStageParameterPayload::Hawkes(payload) => {
                validate_non_empty_artifact_value(
                    payload.selected_candidate_id.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.hawkes.selected_candidate_id",
                    },
                )?;
                validate_hawkes_stage_parameter_boundary(payload)?;
                validate_optimizer_state_presence(self.stage, self.optimizer_state.as_ref())?;
            }
            CalibrationStageParameterPayload::ReturnComponents(payload) => {
                validate_non_empty_artifact_value(
                    payload.selected_candidate_id.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.return_components.selected_candidate_id",
                    },
                )?;
                validate_non_empty_artifact_value(
                    payload.candidate_generation_policy.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.return_components.candidate_generation_policy",
                    },
                )?;
                validate_non_empty_artifact_value(
                    payload.objective_weighting_policy.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.return_components.objective_weighting_policy",
                    },
                )?;
                validate_non_empty_artifact_value(
                    payload.objective_metric_bucket_policy.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.return_components.objective_metric_bucket_policy",
                    },
                )?;
                validate_return_components_stage_parameter_boundary(payload)?;
                validate_optimizer_state_presence(self.stage, self.optimizer_state.as_ref())?;
            }
        }

        if let Some(optimizer_state) = &self.optimizer_state {
            validate_stage_parameter_metadata_field(
                "optimizer_state.schema_version",
                CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION,
                optimizer_state.schema_version.as_str(),
            )?;
            validate_stage_parameter_metadata_field(
                "optimizer_state.optimizer_adapter_metadata.schema_version",
                CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION,
                optimizer_state
                    .optimizer_adapter_metadata
                    .schema_version
                    .as_str(),
            )?;
            validate_non_empty_artifact_value(
                optimizer_state
                    .optimizer_adapter_metadata
                    .backend_id
                    .as_str(),
                CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                    stage: self.stage,
                    error_message: "optimizer adapter metadata backend_id must be non-empty"
                        .to_string(),
                },
            )?;
            validate_non_empty_artifact_value(
                optimizer_state
                    .optimizer_adapter_metadata
                    .selection_policy
                    .as_str(),
                CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                    stage: self.stage,
                    error_message: "optimizer adapter metadata selection_policy must be non-empty"
                        .to_string(),
                },
            )?;
            validate_non_empty_artifact_value(
                optimizer_state
                    .optimizer_adapter_metadata
                    .penalty_policy
                    .as_str(),
                CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                    stage: self.stage,
                    error_message: "optimizer adapter metadata penalty_policy must be non-empty"
                        .to_string(),
                },
            )?;
            validate_non_empty_artifact_value(
                optimizer_state
                    .optimizer_adapter_metadata
                    .bounds_policy
                    .as_str(),
                CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                    stage: self.stage,
                    error_message: "optimizer adapter metadata bounds_policy must be non-empty"
                        .to_string(),
                },
            )?;
            optimizer_state
                .optimizer_adapter_metadata
                .solver_settings
                .validate()
                .map_err(|source| {
                    CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                        stage: self.stage,
                        error_message: source.to_string(),
                    }
                })?;
            if optimizer_state.valid_candidate_count > optimizer_state.input_candidate_count {
                return Err(
                    CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                        stage: self.stage,
                        error_message: "valid_candidate_count must be <= input_candidate_count"
                            .to_string(),
                    },
                );
            }
            if !optimizer_state.selected_candidate_raw_objective.is_finite()
                || !optimizer_state
                    .selected_candidate_penalized_objective
                    .is_finite()
            {
                return Err(
                    CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                        stage: self.stage,
                        error_message: "selected candidate objective values must be finite"
                            .to_string(),
                    },
                );
            }
        }

        Ok(())
    }
}

pub fn build_calibration_stage_parameter_artifact(
    run_contract: &CalibrationRunContract,
    stage_report: &CalibrationStagedRunStageReport,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    if run_contract.run_spec.execution_mode != CalibrationExecutionMode::Disk {
        return Err(
            CalibrationStageParameterArtifactBuildError::UnsupportedExecutionMode {
                expected: CalibrationExecutionMode::Disk,
                found: run_contract.run_spec.execution_mode,
            },
        );
    }

    let stage = match stage_report {
        CalibrationStagedRunStageReport::Seasonality(_) => CalibrationStage::Seasonality,
        CalibrationStagedRunStageReport::Hawkes(_) => CalibrationStage::Hawkes,
        CalibrationStagedRunStageReport::ReturnComponents(_) => CalibrationStage::ReturnComponents,
    };
    let artifact_reference =
        resolve_stage_output_artifact_reference(&run_contract.run_spec, stage)?;

    let artifact = match stage_report {
        CalibrationStagedRunStageReport::Seasonality(report) => {
            build_seasonality_stage_parameter_artifact(report, artifact_reference)
        }
        CalibrationStagedRunStageReport::Hawkes(report) => {
            build_hawkes_stage_parameter_artifact(report, artifact_reference)
        }
        CalibrationStagedRunStageReport::ReturnComponents(report) => {
            build_return_components_stage_parameter_artifact(report, artifact_reference)
        }
    }?;

    artifact.validate().map_err(|source| {
        CalibrationStageParameterArtifactBuildError::ValidationFailed {
            stage,
            error_message: source.to_string(),
        }
    })?;

    Ok(artifact)
}

pub fn calibration_stage_parameter_artifact_path(
    disk_checkpoint_directory: &Path,
    run_id: &str,
    stage: CalibrationStage,
) -> PathBuf {
    disk_checkpoint_directory.join(run_id).join(format!(
        "{}.stage_parameter.artifact.json",
        calibration_stage_token(stage)
    ))
}

pub fn persist_calibration_stage_parameter_artifact_to_disk(
    disk_checkpoint_directory: &Path,
    artifact: &CalibrationStageParameterArtifact,
) -> Result<PathBuf, CalibrationStageParameterArtifactIoError> {
    artifact.validate().map_err(|source| {
        CalibrationStageParameterArtifactIoError::ValidationFailed {
            error_message: source.to_string(),
        }
    })?;

    let artifact_path = calibration_stage_parameter_artifact_path(
        disk_checkpoint_directory,
        artifact.run_id.as_str(),
        artifact.stage,
    );
    let run_directory = artifact_path
        .parent()
        .expect("stage parameter artifact path should always have a parent directory");

    fs::create_dir_all(run_directory).map_err(|source| {
        CalibrationStageParameterArtifactIoError::CreateDirectoryFailed {
            path: run_directory.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    let serialized = serde_json::to_vec_pretty(artifact).map_err(|source| {
        CalibrationStageParameterArtifactIoError::SerializationFailed {
            error_message: source.to_string(),
        }
    })?;

    let temporary_path = artifact_path.with_extension("json.tmp");
    let mut temporary_file = File::create(&temporary_path).map_err(|source| {
        CalibrationStageParameterArtifactIoError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    temporary_file
        .write_all(serialized.as_slice())
        .map_err(
            |source| CalibrationStageParameterArtifactIoError::WriteFailed {
                path: temporary_path.display().to_string(),
                error_message: source.to_string(),
            },
        )?;
    temporary_file.sync_all().map_err(|source| {
        CalibrationStageParameterArtifactIoError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    fs::rename(&temporary_path, &artifact_path).map_err(|source| {
        CalibrationStageParameterArtifactIoError::RenameFailed {
            source_path: temporary_path.display().to_string(),
            target_path: artifact_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    Ok(artifact_path)
}

pub fn load_calibration_stage_parameter_artifact_from_disk(
    artifact_path: &Path,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactIoError> {
    let content = fs::read_to_string(artifact_path).map_err(|source| {
        CalibrationStageParameterArtifactIoError::ReadFailed {
            path: artifact_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    let artifact: CalibrationStageParameterArtifact = serde_json::from_str(content.as_str())
        .map_err(
            |source| CalibrationStageParameterArtifactIoError::DeserializationFailed {
                path: artifact_path.display().to_string(),
                error_message: source.to_string(),
            },
        )?;
    artifact.validate().map_err(|source| {
        CalibrationStageParameterArtifactIoError::ValidationFailed {
            error_message: source.to_string(),
        }
    })?;

    Ok(artifact)
}

pub fn calibration_artifact_manifest_path(
    disk_checkpoint_directory: &Path,
    run_id: &str,
) -> PathBuf {
    disk_checkpoint_directory
        .join(run_id)
        .join("artifact_manifest.json")
}

pub fn build_calibration_artifact_manifest(
    run_contract: &CalibrationRunContract,
    stage_parameter_artifacts: &[CalibrationStageParameterArtifact],
    emitted_checkpoints: &[CalibrationStageCheckpointMetadata],
) -> Result<CalibrationArtifactManifest, CalibrationArtifactManifestBuildError> {
    if run_contract.run_spec.execution_mode != CalibrationExecutionMode::Disk {
        return Err(
            CalibrationArtifactManifestBuildError::UnsupportedExecutionMode {
                expected: CalibrationExecutionMode::Disk,
                found: run_contract.run_spec.execution_mode,
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

    CalibrationArtifactManifest::try_new(run_contract.run_spec.run_id.clone(), artifact_references)
        .map_err(
            |source| CalibrationArtifactManifestBuildError::ManifestValidationFailed {
                error_message: source.to_string(),
            },
        )
}

pub fn persist_calibration_artifact_manifest_to_disk(
    disk_checkpoint_directory: &Path,
    manifest: &CalibrationArtifactManifest,
) -> Result<PathBuf, CalibrationArtifactManifestIoError> {
    manifest.validate().map_err(
        |source| CalibrationArtifactManifestIoError::ValidationFailed {
            error_message: source.to_string(),
        },
    )?;

    let manifest_path =
        calibration_artifact_manifest_path(disk_checkpoint_directory, manifest.run_id.as_str());
    let run_directory = manifest_path
        .parent()
        .expect("artifact manifest path should always have a parent directory");

    fs::create_dir_all(run_directory).map_err(|source| {
        CalibrationArtifactManifestIoError::CreateDirectoryFailed {
            path: run_directory.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    let serialized = serde_json::to_vec_pretty(manifest).map_err(|source| {
        CalibrationArtifactManifestIoError::SerializationFailed {
            error_message: source.to_string(),
        }
    })?;

    let temporary_path = manifest_path.with_extension("json.tmp");
    let mut temporary_file = File::create(&temporary_path).map_err(|source| {
        CalibrationArtifactManifestIoError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    temporary_file
        .write_all(serialized.as_slice())
        .map_err(|source| CalibrationArtifactManifestIoError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        })?;
    temporary_file.sync_all().map_err(|source| {
        CalibrationArtifactManifestIoError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    fs::rename(&temporary_path, &manifest_path).map_err(|source| {
        CalibrationArtifactManifestIoError::RenameFailed {
            source_path: temporary_path.display().to_string(),
            target_path: manifest_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    Ok(manifest_path)
}

pub fn load_calibration_artifact_manifest_from_disk(
    manifest_path: &Path,
) -> Result<CalibrationArtifactManifest, CalibrationArtifactManifestIoError> {
    let content = fs::read_to_string(manifest_path).map_err(|source| {
        CalibrationArtifactManifestIoError::ReadFailed {
            path: manifest_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    let manifest: CalibrationArtifactManifest =
        serde_json::from_str(content.as_str()).map_err(|source| {
            CalibrationArtifactManifestIoError::DeserializationFailed {
                path: manifest_path.display().to_string(),
                error_message: source.to_string(),
            }
        })?;
    manifest.validate().map_err(
        |source| CalibrationArtifactManifestIoError::ValidationFailed {
            error_message: source.to_string(),
        },
    )?;

    Ok(manifest)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationStageParameterArtifactBuildError {
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    StageReportWasNotSuccessful {
        stage: CalibrationStage,
    },
    MissingSuccessfulStagePayload {
        stage: CalibrationStage,
    },
    MissingSuccessfulStageMetadata {
        stage: CalibrationStage,
    },
    ConflictingArtifactReferences {
        stage: CalibrationStage,
        artifact_references: Vec<String>,
    },
    ValidationFailed {
        stage: CalibrationStage,
        error_message: String,
    },
}

impl Display for CalibrationStageParameterArtifactBuildError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedExecutionMode { expected, found } => write!(
                formatter,
                "stage parameter artifacts require execution_mode={expected:?}, found `{found:?}`"
            ),
            Self::StageReportWasNotSuccessful { stage } => write!(
                formatter,
                "cannot build stage parameter artifact for unsuccessful stage `{stage:?}`"
            ),
            Self::MissingSuccessfulStagePayload { stage } => write!(
                formatter,
                "successful stage `{stage:?}` is missing its stage output payload"
            ),
            Self::MissingSuccessfulStageMetadata { stage } => write!(
                formatter,
                "successful stage `{stage:?}` is missing its execution metadata"
            ),
            Self::ConflictingArtifactReferences {
                stage,
                artifact_references,
            } => write!(
                formatter,
                "stage `{stage:?}` has conflicting persisted artifact references {:?}",
                artifact_references
            ),
            Self::ValidationFailed {
                stage,
                error_message,
            } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` failed validation: {error_message}"
            ),
        }
    }
}

impl Error for CalibrationStageParameterArtifactBuildError {}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationStageParameterArtifactValidationError {
    MetadataFieldMismatch {
        field: &'static str,
        expected: &'static str,
        found: String,
    },
    RequiredFieldListMismatch {
        field: &'static str,
        expected: Vec<String>,
        found: Vec<String>,
    },
    EmptyRunId,
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    EmptyArtifactReference {
        stage: CalibrationStage,
    },
    EmptyStageOutputSchemaVersion {
        stage: CalibrationStage,
    },
    EmptySeedMetadataField {
        field: &'static str,
    },
    InvalidSolverSettings {
        stage: CalibrationStage,
        error_message: String,
    },
    StagePayloadMismatch {
        stage: CalibrationStage,
        payload_stage: CalibrationStage,
    },
    EmptyStageParameterField {
        stage: CalibrationStage,
        field: &'static str,
    },
    MissingSeasonalityFactors,
    MissingOptimizerState {
        stage: CalibrationStage,
    },
    UnexpectedOptimizerState {
        stage: CalibrationStage,
    },
    InvalidOptimizerState {
        stage: CalibrationStage,
        error_message: String,
    },
    HawkesBoundaryMismatch {
        expected_transport: &'static str,
    },
    ReturnComponentsBoundaryMismatch {
        expected_transport: &'static str,
    },
}

impl Display for CalibrationStageParameterArtifactValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MetadataFieldMismatch {
                field,
                expected,
                found,
            } => write!(formatter, "{field} must be `{expected}`, found `{found}`"),
            Self::RequiredFieldListMismatch {
                field,
                expected,
                found,
            } => write!(
                formatter,
                "{field} must equal {:?}, found {:?}",
                expected, found
            ),
            Self::EmptyRunId => write!(formatter, "run_id must be a non-empty string"),
            Self::UnsupportedExecutionMode { expected, found } => write!(
                formatter,
                "stage parameter artifact requires execution_mode={expected:?}, found `{found:?}`"
            ),
            Self::EmptyArtifactReference { stage } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` must use a non-empty artifact_reference"
            ),
            Self::EmptyStageOutputSchemaVersion { stage } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` must use a non-empty stage_output_schema_version"
            ),
            Self::EmptySeedMetadataField { field } => {
                write!(formatter, "{field} must be a non-empty string")
            }
            Self::InvalidSolverSettings {
                stage,
                error_message,
            } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` uses invalid solver_settings: {error_message}"
            ),
            Self::StagePayloadMismatch {
                stage,
                payload_stage,
            } => write!(
                formatter,
                "stage parameter artifact stage `{stage:?}` does not match payload stage `{payload_stage:?}`"
            ),
            Self::EmptyStageParameterField { stage, field } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` requires non-empty field `{field}`"
            ),
            Self::MissingSeasonalityFactors => write!(
                formatter,
                "seasonality stage parameter artifact requires non-empty fitted factor vectors"
            ),
            Self::MissingOptimizerState { stage } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` requires optimizer_state metadata"
            ),
            Self::UnexpectedOptimizerState { stage } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` must not include optimizer_state metadata"
            ),
            Self::InvalidOptimizerState {
                stage,
                error_message,
            } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` uses invalid optimizer_state: {error_message}"
            ),
            Self::HawkesBoundaryMismatch { expected_transport } => write!(
                formatter,
                "hawkes stage parameter boundary does not match expected `{expected_transport}` transport contract"
            ),
            Self::ReturnComponentsBoundaryMismatch { expected_transport } => write!(
                formatter,
                "return-components stage parameter boundary does not match expected `{expected_transport}` transport contract"
            ),
        }
    }
}

impl Error for CalibrationStageParameterArtifactValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationStageParameterArtifactIoError {
    CreateDirectoryFailed {
        path: String,
        error_message: String,
    },
    SerializationFailed {
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
    ReadFailed {
        path: String,
        error_message: String,
    },
    DeserializationFailed {
        path: String,
        error_message: String,
    },
    ValidationFailed {
        error_message: String,
    },
}

impl Display for CalibrationStageParameterArtifactIoError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDirectoryFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to create stage parameter artifact directory `{path}`: {error_message}"
            ),
            Self::SerializationFailed { error_message } => write!(
                formatter,
                "failed to serialize stage parameter artifact: {error_message}"
            ),
            Self::WriteFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to write stage parameter artifact `{path}`: {error_message}"
            ),
            Self::RenameFailed {
                source_path,
                target_path,
                error_message,
            } => write!(
                formatter,
                "failed to rename stage parameter artifact `{source_path}` -> `{target_path}`: {error_message}"
            ),
            Self::ReadFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to read stage parameter artifact `{path}`: {error_message}"
            ),
            Self::DeserializationFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to deserialize stage parameter artifact `{path}`: {error_message}"
            ),
            Self::ValidationFailed { error_message } => write!(
                formatter,
                "stage parameter artifact validation failed: {error_message}"
            ),
        }
    }
}

impl Error for CalibrationStageParameterArtifactIoError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationArtifactManifestBuildError {
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationArtifactManifestIoError {
    CreateDirectoryFailed {
        path: String,
        error_message: String,
    },
    SerializationFailed {
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
    ReadFailed {
        path: String,
        error_message: String,
    },
    DeserializationFailed {
        path: String,
        error_message: String,
    },
    ValidationFailed {
        error_message: String,
    },
}

impl Display for CalibrationArtifactManifestIoError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDirectoryFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to create artifact manifest directory `{path}`: {error_message}"
            ),
            Self::SerializationFailed { error_message } => write!(
                formatter,
                "failed to serialize artifact manifest: {error_message}"
            ),
            Self::WriteFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to write artifact manifest `{path}`: {error_message}"
            ),
            Self::RenameFailed {
                source_path,
                target_path,
                error_message,
            } => write!(
                formatter,
                "failed to rename artifact manifest `{source_path}` -> `{target_path}`: {error_message}"
            ),
            Self::ReadFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to read artifact manifest `{path}`: {error_message}"
            ),
            Self::DeserializationFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to deserialize artifact manifest `{path}`: {error_message}"
            ),
            Self::ValidationFailed { error_message } => write!(
                formatter,
                "artifact manifest validation failed: {error_message}"
            ),
        }
    }
}

impl Error for CalibrationArtifactManifestIoError {}

fn build_stage_parameter_output_schema_metadata() -> CalibrationStageParameterOutputSchemaMetadata {
    CalibrationStageParameterOutputSchemaMetadata {
        schema_version: CALIBRATION_STAGE_PARAMETER_OUTPUT_SCHEMA_METADATA_VERSION.to_string(),
        required_artifact_fields: CALIBRATION_STAGE_PARAMETER_REQUIRED_ARTIFACT_FIELDS
            .iter()
            .map(|field| (*field).to_string())
            .collect(),
        required_seed_metadata_fields: CALIBRATION_STAGE_PARAMETER_REQUIRED_SEED_METADATA_FIELDS
            .iter()
            .map(|field| (*field).to_string())
            .collect(),
        required_optimizer_state_fields:
            CALIBRATION_STAGE_PARAMETER_REQUIRED_OPTIMIZER_STATE_FIELDS
                .iter()
                .map(|field| (*field).to_string())
                .collect(),
    }
}

fn build_stage_parameter_seed_metadata(
    run_global_seed: u64,
    stage_seed: u64,
    stage_seed_source: CalibrationStageSeedSource,
    stage_seed_derivation_policy: &str,
) -> CalibrationStageParameterSeedMetadata {
    CalibrationStageParameterSeedMetadata {
        run_global_seed,
        stage_seed,
        stage_seed_source,
        stage_seed_derivation_policy: stage_seed_derivation_policy.to_string(),
    }
}

fn build_hawkes_optimizer_state(
    report: &HawkesStageExecutionReport,
) -> Result<CalibrationStageParameterOptimizerState, CalibrationStageParameterArtifactBuildError> {
    let metadata = report.metadata.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStageMetadata {
            stage: CalibrationStage::Hawkes,
        },
    )?;

    Ok(CalibrationStageParameterOptimizerState {
        schema_version: CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION.to_string(),
        optimizer_adapter_metadata: CalibrationStageParameterOptimizerAdapterMetadata {
            schema_version: metadata
                .optimizer_adapter_metadata
                .schema_version
                .to_string(),
            backend_id: metadata.optimizer_adapter_metadata.backend_id.clone(),
            random_seed: metadata.optimizer_adapter_metadata.random_seed,
            selection_policy: metadata
                .optimizer_adapter_metadata
                .selection_policy
                .to_string(),
            penalty_policy: metadata
                .optimizer_adapter_metadata
                .penalty_policy
                .to_string(),
            bounds_policy: metadata
                .optimizer_adapter_metadata
                .bounds_policy
                .to_string(),
            solver_settings: metadata.optimizer_adapter_metadata.solver_settings.clone(),
        },
        input_candidate_count: metadata.input_candidate_count,
        valid_candidate_count: metadata.valid_candidate_count,
        selected_candidate_raw_objective: metadata.selected_candidate_raw_objective,
        selected_candidate_penalized_objective: metadata.selected_candidate_penalized_objective,
    })
}

fn build_return_components_optimizer_state(
    report: &ReturnComponentsStageExecutionReport,
) -> Result<CalibrationStageParameterOptimizerState, CalibrationStageParameterArtifactBuildError> {
    let metadata = report.metadata.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStageMetadata {
            stage: CalibrationStage::ReturnComponents,
        },
    )?;

    Ok(CalibrationStageParameterOptimizerState {
        schema_version: CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION.to_string(),
        optimizer_adapter_metadata: CalibrationStageParameterOptimizerAdapterMetadata {
            schema_version: metadata
                .optimizer_adapter_metadata
                .schema_version
                .to_string(),
            backend_id: metadata.optimizer_adapter_metadata.backend_id.clone(),
            random_seed: metadata.optimizer_adapter_metadata.random_seed,
            selection_policy: metadata
                .optimizer_adapter_metadata
                .selection_policy
                .to_string(),
            penalty_policy: metadata
                .optimizer_adapter_metadata
                .penalty_policy
                .to_string(),
            bounds_policy: metadata
                .optimizer_adapter_metadata
                .bounds_policy
                .to_string(),
            solver_settings: metadata.optimizer_adapter_metadata.solver_settings.clone(),
        },
        input_candidate_count: metadata.input_candidate_count,
        valid_candidate_count: metadata.valid_candidate_count,
        selected_candidate_raw_objective: metadata.selected_candidate_raw_objective,
        selected_candidate_penalized_objective: metadata.selected_candidate_penalized_objective,
    })
}

fn build_seasonality_stage_parameter_artifact(
    report: &SeasonalityStageExecutionReport,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    if report.status != super::stages::seasonality::CalibrationStageExecutionStatus::Success {
        return Err(
            CalibrationStageParameterArtifactBuildError::StageReportWasNotSuccessful {
                stage: CalibrationStage::Seasonality,
            },
        );
    }

    let payload = report.normalized_stage_payload.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStagePayload {
            stage: CalibrationStage::Seasonality,
        },
    )?;
    let metadata = report.metadata.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStageMetadata {
            stage: CalibrationStage::Seasonality,
        },
    )?;

    Ok(CalibrationStageParameterArtifact {
        schema_version: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: report.run_id.clone(),
        execution_mode: report.execution_mode,
        stage: CalibrationStage::Seasonality,
        artifact_reference,
        schema_metadata: CalibrationStageParameterSchemaMetadata {
            schema_family: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        output_schema_metadata: build_stage_parameter_output_schema_metadata(),
        stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
        seed_metadata: build_stage_parameter_seed_metadata(
            metadata.run_global_seed,
            metadata.stage_seed,
            metadata.stage_seed_source,
            metadata.stage_seed_derivation_policy,
        ),
        solver_settings: metadata.solver_settings.clone(),
        optimizer_state: None,
        stage_parameters: CalibrationStageParameterPayload::Seasonality(
            SeasonalityStageParameterPayload {
                dataset_id: payload.dataset_id.clone(),
                exchange_code: payload.exchange_code.clone(),
                trade_events_data_reference: payload.trade_events_data_reference.clone(),
                estimator_version: metadata.estimator_version.clone(),
                timestamp_event_input_schema_version: payload
                    .timestamp_event_input_schema_version
                    .clone(),
                timestamp_event_preprocessed_schema_version: payload
                    .timestamp_event_preprocessed_schema_version
                    .clone(),
                intraday_estimate_schema_version: payload.intraday_estimate_schema_version.clone(),
                day_of_week_estimate_schema_version: payload
                    .day_of_week_estimate_schema_version
                    .clone(),
                day_of_year_estimate_schema_version: payload
                    .day_of_year_estimate_schema_version
                    .clone(),
                composition_schema_version: payload.composition_schema_version.clone(),
                intraday_minutes_per_bin: payload.intraday_minutes_per_bin,
                intraday_factor_by_bin: payload.intraday_factor_by_bin.clone(),
                day_of_week_factor: payload.day_of_week_factor.clone(),
                day_of_year_factor: payload.day_of_year_factor.clone(),
                normalization_metadata: payload.normalization_metadata.clone(),
            },
        ),
    })
}

fn build_hawkes_stage_parameter_artifact(
    report: &HawkesStageExecutionReport,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    if report.status != super::stages::seasonality::CalibrationStageExecutionStatus::Success {
        return Err(
            CalibrationStageParameterArtifactBuildError::StageReportWasNotSuccessful {
                stage: CalibrationStage::Hawkes,
            },
        );
    }

    let payload = report.fitted_stage_payload.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStagePayload {
            stage: CalibrationStage::Hawkes,
        },
    )?;
    let metadata = report.metadata.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStageMetadata {
            stage: CalibrationStage::Hawkes,
        },
    )?;

    Ok(CalibrationStageParameterArtifact {
        schema_version: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: report.run_id.clone(),
        execution_mode: report.execution_mode,
        stage: CalibrationStage::Hawkes,
        artifact_reference,
        schema_metadata: CalibrationStageParameterSchemaMetadata {
            schema_family: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        output_schema_metadata: build_stage_parameter_output_schema_metadata(),
        stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
        seed_metadata: build_stage_parameter_seed_metadata(
            metadata.run_global_seed,
            metadata.stage_seed,
            metadata.stage_seed_source,
            metadata.stage_seed_derivation_policy,
        ),
        solver_settings: metadata.solver_settings.clone(),
        optimizer_state: Some(build_hawkes_optimizer_state(report)?),
        stage_parameters: CalibrationStageParameterPayload::Hawkes(HawkesStageParameterPayload {
            selected_candidate_id: payload.selected_candidate_id.clone(),
            fitted_parameters: payload.fitted_parameters.clone(),
            seasonality_input_transport: payload.seasonality_input_transport,
            seasonality_dataset_id: payload.seasonality_dataset_id.clone(),
            seasonality_artifact_reference: payload.seasonality_artifact_reference.clone(),
        }),
    })
}

fn build_return_components_stage_parameter_artifact(
    report: &ReturnComponentsStageExecutionReport,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    if report.status != super::stages::seasonality::CalibrationStageExecutionStatus::Success {
        return Err(
            CalibrationStageParameterArtifactBuildError::StageReportWasNotSuccessful {
                stage: CalibrationStage::ReturnComponents,
            },
        );
    }

    let payload = report.fitted_stage_payload.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStagePayload {
            stage: CalibrationStage::ReturnComponents,
        },
    )?;
    let metadata = report.metadata.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStageMetadata {
            stage: CalibrationStage::ReturnComponents,
        },
    )?;

    Ok(CalibrationStageParameterArtifact {
        schema_version: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: report.run_id.clone(),
        execution_mode: report.execution_mode,
        stage: CalibrationStage::ReturnComponents,
        artifact_reference,
        schema_metadata: CalibrationStageParameterSchemaMetadata {
            schema_family: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        output_schema_metadata: build_stage_parameter_output_schema_metadata(),
        stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
        seed_metadata: build_stage_parameter_seed_metadata(
            metadata.run_global_seed,
            metadata.stage_seed,
            metadata.stage_seed_source,
            metadata.stage_seed_derivation_policy,
        ),
        solver_settings: metadata.solver_settings.clone(),
        optimizer_state: Some(build_return_components_optimizer_state(report)?),
        stage_parameters: CalibrationStageParameterPayload::ReturnComponents(
            ReturnComponentsStageParameterPayload {
                selected_candidate_id: payload.selected_candidate_id.clone(),
                fitted_parameters: payload.fitted_parameters.clone(),
                candidate_generation_policy: payload.candidate_generation_policy.to_string(),
                objective_weighting_policy: payload.objective_weighting_policy.to_string(),
                objective_metric_bucket_policy: payload.objective_metric_bucket_policy.to_string(),
                activity_input_transport: payload.activity_input_transport,
                activity_selected_hawkes_candidate_id: payload
                    .activity_selected_hawkes_candidate_id
                    .clone(),
                activity_artifact_reference: payload.activity_artifact_reference.clone(),
            },
        ),
    })
}

fn resolve_stage_output_artifact_reference(
    run_spec: &super::contracts::CalibrationRunSpec,
    stage: CalibrationStage,
) -> Result<String, CalibrationStageParameterArtifactBuildError> {
    let mut artifact_references = run_spec
        .stage_handoffs
        .iter()
        .filter(|handoff| handoff.from_stage == stage)
        .filter_map(|handoff| handoff.artifact_reference())
        .filter(|artifact_reference| !artifact_reference.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    artifact_references.sort();
    artifact_references.dedup();

    match artifact_references.as_slice() {
        [] => Ok(format!(
            "artifact://{}/{}",
            calibration_stage_token(stage),
            run_spec.run_id
        )),
        [artifact_reference] => Ok(artifact_reference.clone()),
        _ => Err(
            CalibrationStageParameterArtifactBuildError::ConflictingArtifactReferences {
                stage,
                artifact_references,
            },
        ),
    }
}

fn validate_stage_parameter_metadata_field(
    field: &'static str,
    expected: &'static str,
    found: &str,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    if found != expected {
        return Err(
            CalibrationStageParameterArtifactValidationError::MetadataFieldMismatch {
                field,
                expected,
                found: found.to_string(),
            },
        );
    }

    Ok(())
}

fn validate_stage_parameter_required_string_list(
    field: &'static str,
    expected: &[&str],
    found: &[String],
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    let expected_fields: Vec<String> = expected.iter().map(|value| (*value).to_string()).collect();
    if found != expected_fields.as_slice() {
        return Err(
            CalibrationStageParameterArtifactValidationError::RequiredFieldListMismatch {
                field,
                expected: expected_fields,
                found: found.to_vec(),
            },
        );
    }

    Ok(())
}

fn validate_non_empty_artifact_value<E>(value: &str, error: E) -> Result<(), E> {
    if value.trim().is_empty() {
        return Err(error);
    }

    Ok(())
}

fn validate_optimizer_state_presence(
    stage: CalibrationStage,
    optimizer_state: Option<&CalibrationStageParameterOptimizerState>,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    if optimizer_state.is_none() {
        return Err(
            CalibrationStageParameterArtifactValidationError::MissingOptimizerState { stage },
        );
    }

    Ok(())
}

fn validate_hawkes_stage_parameter_boundary(
    payload: &HawkesStageParameterPayload,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    match payload.seasonality_input_transport {
        super::contracts::CalibrationStageHandoffTransport::InMemoryOutput => {
            if payload
                .seasonality_dataset_id
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
                || payload.seasonality_artifact_reference.is_some()
            {
                return Err(
                    CalibrationStageParameterArtifactValidationError::HawkesBoundaryMismatch {
                        expected_transport: "in_memory_output",
                    },
                );
            }
        }
        super::contracts::CalibrationStageHandoffTransport::PersistedArtifactReference => {
            if payload
                .seasonality_artifact_reference
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
                || payload.seasonality_dataset_id.is_some()
            {
                return Err(
                    CalibrationStageParameterArtifactValidationError::HawkesBoundaryMismatch {
                        expected_transport: "persisted_artifact_reference",
                    },
                );
            }
        }
    }

    Ok(())
}

fn validate_return_components_stage_parameter_boundary(
    payload: &ReturnComponentsStageParameterPayload,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    match payload.activity_input_transport {
        super::contracts::CalibrationStageHandoffTransport::InMemoryOutput => {
            if payload
                .activity_selected_hawkes_candidate_id
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
                || payload.activity_artifact_reference.is_some()
            {
                return Err(
                    CalibrationStageParameterArtifactValidationError::ReturnComponentsBoundaryMismatch {
                        expected_transport: "in_memory_output",
                    },
                );
            }
        }
        super::contracts::CalibrationStageHandoffTransport::PersistedArtifactReference => {
            if payload
                .activity_artifact_reference
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
                || payload.activity_selected_hawkes_candidate_id.is_some()
            {
                return Err(
                    CalibrationStageParameterArtifactValidationError::ReturnComponentsBoundaryMismatch {
                        expected_transport: "persisted_artifact_reference",
                    },
                );
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationDatasetSnapshotArtifactValidationError {
    MetadataFieldMismatch {
        field: &'static str,
        expected: &'static str,
        found: String,
    },
    RequiredFieldListMismatch {
        field: &'static str,
        expected: Vec<String>,
        found: Vec<String>,
    },
    EmptyRunId,
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    EmptyExchangeCode,
    EmptyProvenanceField {
        field: &'static str,
    },
    MissingInputDatasets,
    DuplicateInputDataset {
        input_name: CalibrationDatasetInputName,
    },
    MissingRequiredInputDataset {
        input_name: CalibrationDatasetInputName,
    },
    EmptyInputDataReference {
        input_name: CalibrationDatasetInputName,
    },
    InputDatasetStageMismatch {
        input_name: CalibrationDatasetInputName,
        expected: CalibrationStage,
        found: CalibrationStage,
    },
    InvalidReferenceChecksumFormat {
        input_name: CalibrationDatasetInputName,
        found: String,
    },
    ReferenceChecksumMismatch {
        input_name: CalibrationDatasetInputName,
        expected: String,
        found: String,
    },
    InvalidSnapshotChecksumFormat {
        found: String,
    },
    SnapshotChecksumMismatch {
        expected: String,
        found: String,
    },
}

impl Display for CalibrationDatasetSnapshotArtifactValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MetadataFieldMismatch {
                field,
                expected,
                found,
            } => {
                write!(formatter, "{field} must be `{expected}`, found `{found}`")
            }
            Self::RequiredFieldListMismatch {
                field,
                expected,
                found,
            } => {
                write!(
                    formatter,
                    "{field} must equal {:?}, found {:?}",
                    expected, found
                )
            }
            Self::EmptyRunId => write!(formatter, "run_id must be a non-empty string"),
            Self::UnsupportedExecutionMode { expected, found } => write!(
                formatter,
                "dataset snapshot artifact requires execution_mode={expected:?}, found `{found:?}`"
            ),
            Self::EmptyExchangeCode => {
                write!(formatter, "exchange_code must be a non-empty string")
            }
            Self::EmptyProvenanceField { field } => {
                write!(formatter, "{field} must be a non-empty string")
            }
            Self::MissingInputDatasets => write!(
                formatter,
                "input_datasets must contain at least one dataset reference"
            ),
            Self::DuplicateInputDataset { input_name } => write!(
                formatter,
                "input_datasets contains duplicate input_name `{}`",
                input_name.as_str()
            ),
            Self::MissingRequiredInputDataset { input_name } => write!(
                formatter,
                "input_datasets is missing required input `{}`",
                input_name.as_str()
            ),
            Self::EmptyInputDataReference { input_name } => write!(
                formatter,
                "input_datasets entry `{}` must use a non-empty data_reference",
                input_name.as_str()
            ),
            Self::InputDatasetStageMismatch {
                input_name,
                expected,
                found,
            } => write!(
                formatter,
                "input_datasets entry `{}` must use primary_stage=`{}`, found `{}`",
                input_name.as_str(),
                calibration_stage_token(*expected),
                calibration_stage_token(*found)
            ),
            Self::InvalidReferenceChecksumFormat { input_name, found } => write!(
                formatter,
                "input_datasets entry `{}` uses invalid reference_checksum `{found}`",
                input_name.as_str()
            ),
            Self::ReferenceChecksumMismatch {
                input_name,
                expected,
                found,
            } => write!(
                formatter,
                "input_datasets entry `{}` has inconsistent reference_checksum, expected `{expected}`, found `{found}`",
                input_name.as_str()
            ),
            Self::InvalidSnapshotChecksumFormat { found } => write!(
                formatter,
                "snapshot_checksum must be a lowercase fnv1a hex string, found `{found}`"
            ),
            Self::SnapshotChecksumMismatch { expected, found } => write!(
                formatter,
                "snapshot_checksum is inconsistent, expected `{expected}`, found `{found}`"
            ),
        }
    }
}

impl Error for CalibrationDatasetSnapshotArtifactValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationDatasetSnapshotArtifactIoError {
    CreateDirectoryFailed {
        path: String,
        error_message: String,
    },
    SerializationFailed {
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
    ReadFailed {
        path: String,
        error_message: String,
    },
    DeserializationFailed {
        path: String,
        error_message: String,
    },
    ValidationFailed {
        error_message: String,
    },
}

impl Display for CalibrationDatasetSnapshotArtifactIoError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDirectoryFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to create dataset snapshot artifact directory `{path}`: {error_message}"
            ),
            Self::SerializationFailed { error_message } => write!(
                formatter,
                "failed to serialize dataset snapshot artifact: {error_message}"
            ),
            Self::WriteFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to write dataset snapshot artifact `{path}`: {error_message}"
            ),
            Self::RenameFailed {
                source_path,
                target_path,
                error_message,
            } => write!(
                formatter,
                "failed to rename dataset snapshot artifact `{source_path}` -> `{target_path}`: {error_message}"
            ),
            Self::ReadFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to read dataset snapshot artifact `{path}`: {error_message}"
            ),
            Self::DeserializationFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to deserialize dataset snapshot artifact `{path}`: {error_message}"
            ),
            Self::ValidationFailed { error_message } => write!(
                formatter,
                "dataset snapshot artifact validation failed: {error_message}"
            ),
        }
    }
}

impl Error for CalibrationDatasetSnapshotArtifactIoError {}

fn calibration_execution_mode_token(execution_mode: CalibrationExecutionMode) -> &'static str {
    match execution_mode {
        CalibrationExecutionMode::Memory => "memory",
        CalibrationExecutionMode::Disk => "disk",
    }
}

fn calibration_stage_token(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "seasonality",
        CalibrationStage::Hawkes => "hawkes",
        CalibrationStage::ReturnComponents => "return_components",
    }
}

fn compute_reference_checksum_hex(data_reference: &str) -> String {
    format!("{:016x}", fnv1a_64(data_reference.as_bytes()))
}

fn validate_exact_metadata_field(
    field: &'static str,
    expected: &'static str,
    found: &str,
) -> Result<(), CalibrationDatasetSnapshotArtifactValidationError> {
    if found != expected {
        return Err(
            CalibrationDatasetSnapshotArtifactValidationError::MetadataFieldMismatch {
                field,
                expected,
                found: found.to_string(),
            },
        );
    }

    Ok(())
}

fn validate_non_empty_provenance_field(
    field: &'static str,
    value: &str,
) -> Result<(), CalibrationDatasetSnapshotArtifactValidationError> {
    if value.trim().is_empty() {
        return Err(
            CalibrationDatasetSnapshotArtifactValidationError::EmptyProvenanceField { field },
        );
    }

    Ok(())
}

fn validate_exact_string_list(
    field: &'static str,
    expected: &[&str],
    found: &[String],
) -> Result<(), CalibrationDatasetSnapshotArtifactValidationError> {
    let expected_fields: Vec<String> = expected.iter().map(|value| (*value).to_string()).collect();
    if found != expected_fields.as_slice() {
        return Err(
            CalibrationDatasetSnapshotArtifactValidationError::RequiredFieldListMismatch {
                field,
                expected: expected_fields,
                found: found.to_vec(),
            },
        );
    }

    Ok(())
}

fn validate_checksum_hex<E>(checksum_hex: &str, error: E) -> Result<(), E> {
    if checksum_hex.len() != 16
        || checksum_hex
            .chars()
            .any(|character| !matches!(character, '0'..='9' | 'a'..='f'))
    {
        return Err(error);
    }

    Ok(())
}

fn write_json_file_atomically<T: Serialize>(
    target_path: &Path,
    payload: &T,
) -> Result<(), CalibrationDatasetSnapshotArtifactIoError> {
    let serialized = serde_json::to_vec_pretty(payload).map_err(|source| {
        CalibrationDatasetSnapshotArtifactIoError::SerializationFailed {
            error_message: source.to_string(),
        }
    })?;

    let temporary_path = target_path.with_extension("json.tmp");
    let mut temporary_file = File::create(&temporary_path).map_err(|source| {
        CalibrationDatasetSnapshotArtifactIoError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;
    temporary_file
        .write_all(serialized.as_slice())
        .map_err(
            |source| CalibrationDatasetSnapshotArtifactIoError::WriteFailed {
                path: temporary_path.display().to_string(),
                error_message: source.to_string(),
            },
        )?;
    temporary_file.sync_all().map_err(|source| {
        CalibrationDatasetSnapshotArtifactIoError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    fs::rename(&temporary_path, target_path).map_err(|source| {
        CalibrationDatasetSnapshotArtifactIoError::RenameFailed {
            source_path: temporary_path.display().to_string(),
            target_path: target_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    Ok(())
}
