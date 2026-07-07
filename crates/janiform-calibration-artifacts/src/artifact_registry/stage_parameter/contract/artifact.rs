use serde::{Deserialize, Serialize};

use super::super::super::super::contracts::{
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationStage,
};
use super::super::super::dataset_snapshot::CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY;
use super::errors::CalibrationStageParameterArtifactValidationError;
use super::hawkes::validate_hawkes_stage_parameter_payload;
use super::joint_refinement::validate_joint_refinement_stage_parameter_payload;
use super::long_variance::validate_long_variance_stage_parameter_payload;
use super::mean_reversion::validate_mean_reversion_stage_parameter_payload;
use super::metadata::CalibrationStageParameterBuildMetadata;
use super::news_shocks::validate_news_shocks_stage_parameter_payload;
use super::optimizer::{
    CalibrationStageParameterOptimizerState, CalibrationStageParameterOptimizerStateInput,
    build_optimizer_state, validate_optimizer_state,
};
use super::payload::CalibrationStageParameterPayload;
use super::return_backfit::validate_return_backfit_stage_parameter_payload;
use super::return_components::validate_return_components_stage_parameter_payload;
use super::schema::{
    CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY,
    CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION,
    CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION,
    CALIBRATION_STAGE_PARAMETER_OUTPUT_SCHEMA_METADATA_VERSION,
    CALIBRATION_STAGE_PARAMETER_REQUIRED_ARTIFACT_FIELDS,
    CALIBRATION_STAGE_PARAMETER_REQUIRED_OPTIMIZER_STATE_FIELDS,
    CALIBRATION_STAGE_PARAMETER_REQUIRED_SEED_METADATA_FIELDS,
    CalibrationStageParameterOutputSchemaMetadata, CalibrationStageParameterSchemaMetadata,
    CalibrationStageParameterSeedMetadata, build_stage_parameter_output_schema_metadata,
    build_stage_parameter_schema_metadata, build_stage_parameter_seed_metadata,
};
use super::seasonality::validate_seasonality_stage_parameter_payload;
use super::support::{
    validate_stage_parameter_metadata_field, validate_stage_parameter_required_string_list,
};

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
    pub solver_settings: CalibrationOptimizerSettings,
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

        self.validate_schema_metadata()?;

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

        self.validate_stage_payload()?;

        if let Some(optimizer_state) = &self.optimizer_state {
            validate_optimizer_state(self.stage, optimizer_state)?;
        }

        Ok(())
    }

    fn validate_schema_metadata(
        &self,
    ) -> Result<(), CalibrationStageParameterArtifactValidationError> {
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
        )
    }

    fn validate_stage_payload(
        &self,
    ) -> Result<(), CalibrationStageParameterArtifactValidationError> {
        match &self.stage_parameters {
            CalibrationStageParameterPayload::Seasonality(payload) => {
                validate_seasonality_stage_parameter_payload(
                    self.stage,
                    payload,
                    self.optimizer_state.as_ref(),
                )
            }
            CalibrationStageParameterPayload::Hawkes(payload) => {
                validate_hawkes_stage_parameter_payload(
                    self.stage,
                    payload,
                    self.optimizer_state.as_ref(),
                )
            }
            CalibrationStageParameterPayload::ReturnComponents(payload) => {
                validate_return_components_stage_parameter_payload(
                    self.stage,
                    payload,
                    self.optimizer_state.as_ref(),
                )
            }
            CalibrationStageParameterPayload::LongVariance(payload) => {
                validate_long_variance_stage_parameter_payload(
                    self.stage,
                    payload,
                    self.optimizer_state.as_ref(),
                )
            }
            CalibrationStageParameterPayload::NewsShocks(payload) => {
                validate_news_shocks_stage_parameter_payload(
                    self.stage,
                    payload,
                    self.optimizer_state.as_ref(),
                )
            }
            CalibrationStageParameterPayload::ReturnBackfit(payload) => {
                validate_return_backfit_stage_parameter_payload(
                    self.stage,
                    payload,
                    self.optimizer_state.as_ref(),
                )
            }
            CalibrationStageParameterPayload::MeanReversion(payload) => {
                validate_mean_reversion_stage_parameter_payload(
                    self.stage,
                    payload,
                    self.optimizer_state.as_ref(),
                )
            }
            CalibrationStageParameterPayload::JointRefinement(payload)
            | CalibrationStageParameterPayload::OptionRefinement(payload)
            | CalibrationStageParameterPayload::Finalization(payload) => {
                validate_joint_refinement_stage_parameter_payload(
                    self.stage,
                    payload,
                    self.optimizer_state.as_ref(),
                )
            }
        }
    }
}

pub(super) fn build_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    stage: CalibrationStage,
    artifact_reference: String,
    metadata: CalibrationStageParameterBuildMetadata,
    optimizer_state: Option<CalibrationStageParameterOptimizerStateInput>,
    stage_parameters: CalibrationStageParameterPayload,
) -> CalibrationStageParameterArtifact {
    CalibrationStageParameterArtifact {
        schema_version: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: run_id.to_string(),
        execution_mode,
        stage,
        artifact_reference,
        schema_metadata: build_stage_parameter_schema_metadata(),
        output_schema_metadata: build_stage_parameter_output_schema_metadata(),
        stage_output_schema_version: metadata.stage_output_schema_version,
        seed_metadata: build_stage_parameter_seed_metadata(
            metadata.run_global_seed,
            metadata.stage_seed,
            metadata.stage_seed_source,
            metadata.stage_seed_derivation_policy.as_str(),
        ),
        solver_settings: metadata.solver_settings,
        optimizer_state: optimizer_state.map(build_optimizer_state),
        stage_parameters,
    }
}
