use serde::{Deserialize, Serialize};

use janiform_calibration_artifacts::{
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationStage,
    CalibrationStageSeedSource, HawkesStageFittedParameters, ObservationModelsStageDiagnostics,
};
use janiform_model_kernels::return_clock::SideSourceActivityDecomposition;
use janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModel;

use crate::stages::return_components::ReturnComponentsObservedOpenSessionReturn;
use crate::stages::seasonality::{
    CalibrationStageExecutionStatus, SeasonalityStageNormalizedPayload,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationModelsOpenSessionTimestampTransition {
    pub previous_timestamp_utc_nanos: i64,
    pub current_timestamp_utc_nanos: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObservationModelsStageExecutionInput {
    pub observed_open_session_returns: Vec<ReturnComponentsObservedOpenSessionReturn>,
    pub open_session_timestamp_transitions: Vec<ObservationModelsOpenSessionTimestampTransition>,
    pub source_responsibility_input: Option<ObservationModelsHawkesSourceResponsibilityInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObservationModelsHawkesSourceResponsibilityInput {
    pub seasonality_payload: SeasonalityStageNormalizedPayload,
    pub event_side_codes: Vec<u8>,
    pub hawkes_fitted_parameters: HawkesStageFittedParameters,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObservationModelsStageFittedPayload {
    pub schema_version: &'static str,
    pub timestamp_batching_observation_model: TimestampBatchingObservationModel,
    pub open_session_return_source_activities: Vec<SideSourceActivityDecomposition>,
    pub diagnostics: ObservationModelsStageDiagnostics,
}

impl ObservationModelsStageFittedPayload {
    pub(crate) fn without_source_activity_vector(&self) -> Self {
        Self {
            schema_version: self.schema_version,
            timestamp_batching_observation_model: self.timestamp_batching_observation_model.clone(),
            open_session_return_source_activities: Vec::new(),
            diagnostics: self.diagnostics.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObservationModelsStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
    pub observed_open_session_return_count: usize,
    pub same_session_timestamp_transition_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum ObservationModelsStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingObservedOpenSessionReturns,
    MissingHawkesSourceResponsibilityInput,
    InvalidHawkesSourceResponsibilityInput { error_message: String },
    InvalidTimestampBatchingObservationModel { error_message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObservationModelsStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<ObservationModelsStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ObservationModelsStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<ObservationModelsStageExecutionFailure>,
}
