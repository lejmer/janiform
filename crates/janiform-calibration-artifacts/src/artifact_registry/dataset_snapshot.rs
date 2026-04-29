use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use janiform_foundations::strings::is_non_empty_string;
use serde::{Deserialize, Serialize};

use super::super::contracts::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY,
    CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CALIBRATION_RUN_DATA_INPUTS_SCHEMA_VERSION,
    CALIBRATION_RUN_SPEC_SCHEMA_VERSION, CalibrationExecutionMode, CalibrationRunContract,
    CalibrationStage,
};
use super::shared::{
    CalibrationArtifactIoError, CalibrationArtifactPersistenceKind,
    calibration_committed_artifact_run_directory, calibration_execution_mode_token,
    calibration_stage_token, compute_reference_checksum_hex, load_validated_json_artifact,
    persist_validated_json_artifact, validate_checksum_hex,
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
    persistence_root_directory: &Path,
    run_id: &str,
) -> PathBuf {
    let run_directory =
        calibration_committed_artifact_run_directory(persistence_root_directory, run_id);
    calibration_dataset_snapshot_artifact_path_in_run_directory(run_directory.as_path())
}

pub(crate) fn calibration_dataset_snapshot_artifact_path_in_run_directory(
    run_directory: &Path,
) -> PathBuf {
    run_directory.join("dataset_snapshot.artifact.json")
}

pub fn persist_calibration_dataset_snapshot_artifact_to_disk(
    persistence_root_directory: &Path,
    artifact: &CalibrationDatasetSnapshotArtifact,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::DatasetSnapshot;
    let artifact_path = calibration_dataset_snapshot_artifact_path(
        persistence_root_directory,
        artifact.run_id.as_str(),
    );
    persist_validated_json_artifact(
        artifact_kind,
        artifact_path.as_path(),
        artifact,
        |payload| payload.validate().map_err(|source| source.to_string()),
    )
}

pub fn persist_calibration_dataset_snapshot_artifact_to_run_directory(
    run_directory: &Path,
    artifact: &CalibrationDatasetSnapshotArtifact,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::DatasetSnapshot;
    let artifact_path = calibration_dataset_snapshot_artifact_path_in_run_directory(run_directory);

    persist_validated_json_artifact(
        artifact_kind,
        artifact_path.as_path(),
        artifact,
        |payload| payload.validate().map_err(|source| source.to_string()),
    )
}

pub fn persist_calibration_dataset_snapshot_from_run_contract_to_disk(
    persistence_root_directory: &Path,
    run_contract: &CalibrationRunContract,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::DatasetSnapshot;
    let artifact = build_calibration_dataset_snapshot_artifact(run_contract).map_err(|source| {
        CalibrationArtifactIoError::validation_failed(artifact_kind, source.to_string())
    })?;

    persist_calibration_dataset_snapshot_artifact_to_disk(persistence_root_directory, &artifact)
}

pub fn load_calibration_dataset_snapshot_artifact_from_disk(
    artifact_path: &Path,
) -> Result<CalibrationDatasetSnapshotArtifact, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::DatasetSnapshot;

    load_validated_json_artifact(
        artifact_kind,
        artifact_path,
        |payload: &CalibrationDatasetSnapshotArtifact| {
            payload.validate().map_err(|source| source.to_string())
        },
    )
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
    if !is_non_empty_string(value) {
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
