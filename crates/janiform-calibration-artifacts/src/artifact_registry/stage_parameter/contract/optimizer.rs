use serde::{Deserialize, Serialize};

use super::super::super::super::contracts::{CalibrationOptimizerSettings, CalibrationStage};
use super::errors::CalibrationStageParameterArtifactValidationError;
use super::schema::{
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION,
    CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION,
};
use super::support::{validate_non_empty_artifact_value, validate_stage_parameter_metadata_field};

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
    pub solver_settings: CalibrationOptimizerSettings,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationStageParameterOptimizerStateInput {
    pub backend_id: String,
    pub random_seed: u64,
    pub selection_policy: String,
    pub penalty_policy: String,
    pub bounds_policy: String,
    pub solver_settings: CalibrationOptimizerSettings,
    pub input_candidate_count: usize,
    pub valid_candidate_count: usize,
    pub selected_candidate_raw_objective: f64,
    pub selected_candidate_penalized_objective: f64,
}

pub(super) fn build_optimizer_state(
    optimizer_state: CalibrationStageParameterOptimizerStateInput,
) -> CalibrationStageParameterOptimizerState {
    CalibrationStageParameterOptimizerState {
        schema_version: CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION.to_string(),
        optimizer_adapter_metadata: CalibrationStageParameterOptimizerAdapterMetadata {
            schema_version: CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION
                .to_string(),
            backend_id: optimizer_state.backend_id,
            random_seed: optimizer_state.random_seed,
            selection_policy: optimizer_state.selection_policy,
            penalty_policy: optimizer_state.penalty_policy,
            bounds_policy: optimizer_state.bounds_policy,
            solver_settings: optimizer_state.solver_settings,
        },
        input_candidate_count: optimizer_state.input_candidate_count,
        valid_candidate_count: optimizer_state.valid_candidate_count,
        selected_candidate_raw_objective: optimizer_state.selected_candidate_raw_objective,
        selected_candidate_penalized_objective: optimizer_state
            .selected_candidate_penalized_objective,
    }
}

pub(super) fn validate_optimizer_state(
    stage: CalibrationStage,
    optimizer_state: &CalibrationStageParameterOptimizerState,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
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
            stage,
            error_message: "optimizer adapter metadata backend_id must be non-empty".to_string(),
        },
    )?;

    validate_non_empty_artifact_value(
        optimizer_state
            .optimizer_adapter_metadata
            .selection_policy
            .as_str(),
        CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
            stage,
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
            stage,
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
            stage,
            error_message: "optimizer adapter metadata bounds_policy must be non-empty".to_string(),
        },
    )?;

    optimizer_state
        .optimizer_adapter_metadata
        .solver_settings
        .validate()
        .map_err(|source| {
            CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                stage,
                error_message: source.to_string(),
            }
        })?;

    if optimizer_state.valid_candidate_count > optimizer_state.input_candidate_count {
        return Err(
            CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                stage,
                error_message: "valid_candidate_count must be <= input_candidate_count".to_string(),
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
                stage,
                error_message: "selected candidate objective values must be finite".to_string(),
            },
        );
    }

    Ok(())
}
