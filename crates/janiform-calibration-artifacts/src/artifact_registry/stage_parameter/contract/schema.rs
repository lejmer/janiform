use serde::{Deserialize, Serialize};

use super::super::super::super::contracts::CalibrationStageSeedSource;
use super::super::super::dataset_snapshot::CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY;

pub const CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION: &str =
    "calibration.artifact_registry.stage_parameter.v1";
pub const CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY: &str =
    "calibration.artifact_registry.stage_parameter";
pub const CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION: &str = "v1";
pub const CALIBRATION_STAGE_PARAMETER_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "calibration.artifact_registry.stage_parameter.output_schema_metadata.v1";
pub const CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION: &str =
    "calibration.artifact_registry.stage_parameter.optimizer_state.v1";
pub const CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION: &str =
    "calibration.optimizer.adapter_run.v1";

pub(super) const CALIBRATION_STAGE_PARAMETER_REQUIRED_ARTIFACT_FIELDS: [&str; 11] = [
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
pub(super) const CALIBRATION_STAGE_PARAMETER_REQUIRED_SEED_METADATA_FIELDS: [&str; 4] = [
    "run_global_seed",
    "stage_seed",
    "stage_seed_source",
    "stage_seed_derivation_policy",
];
pub(super) const CALIBRATION_STAGE_PARAMETER_REQUIRED_OPTIMIZER_STATE_FIELDS: [&str; 6] = [
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

pub(super) fn build_stage_parameter_schema_metadata() -> CalibrationStageParameterSchemaMetadata {
    CalibrationStageParameterSchemaMetadata {
        schema_family: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY.to_string(),
        schema_generation: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION.to_string(),
        forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY.to_string(),
    }
}

pub(super) fn build_stage_parameter_output_schema_metadata()
-> CalibrationStageParameterOutputSchemaMetadata {
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

pub(super) fn build_stage_parameter_seed_metadata(
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
