mod checkpointing;
mod contracts;
mod dispatch;
mod execution;
mod observer;
mod payload_state;
mod reporting;

pub(super) use crate::checkpointing::CalibrationInMemoryStagePayloadState;
pub(super) use crate::diagnostics::types::{
    CalibrationDiagnosticArtifactBundle, CalibrationDiagnosticArtifactReference,
    CalibrationDiagnosticRuntimeProfileEntry,
};
pub(super) use crate::stages::hawkes::{
    HawkesStageCandidateInput, HawkesStageExecutionInput, HawkesStageExecutionMetadata,
    HawkesStageExecutionReport, HawkesStageSeasonalityInput, run_hawkes_calibration_stage,
};
pub(super) use crate::stages::joint_refinement::{
    JointRefinementStageExecutionInput, JointRefinementStageExecutionMetadata,
    JointRefinementStageExecutionReport, JointRefinementStageHawkesInput,
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
    JointRefinementStageReturnComponentsInput, JointRefinementStageSeasonalityFactorsInput,
    run_joint_refinement_calibration_stage,
    run_joint_refinement_calibration_stage_with_progress_observer,
};
pub(super) use crate::stages::mean_reversion::{
    MeanReversionStageExecutionInput, MeanReversionStageExecutionMetadata,
    MeanReversionStageExecutionReport, MeanReversionStageReturnComponentsInput,
    run_mean_reversion_calibration_stage,
};
pub(super) use crate::stages::return_components::{
    ReturnComponentsStageActivityInput, ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageExecutionInput, ReturnComponentsStageExecutionMetadata,
    ReturnComponentsStageExecutionReport, run_return_components_calibration_stage,
};
pub(super) use crate::stages::seasonality::{
    CalibrationStageExecutionStatus, SeasonalityStageExecutionInput,
    SeasonalityStageExecutionMetadata, SeasonalityStageExecutionReport,
    run_seasonality_calibration_stage,
};
pub(super) use janiform_calibration_artifacts::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY,
    CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY, CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION,
    CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY, CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationRunContract,
    CalibrationRunSpec, CalibrationStage, CalibrationStageCheckpointMetadata,
    CalibrationStageHandoffTransport, CalibrationStageParameterArtifact,
    CalibrationStageParameterArtifactInput, CalibrationStageParameterBuildMetadata,
    CalibrationStageParameterOptimizerStateInput, CalibrationStageParameterPayload,
    CalibrationStageSeedSource, stage_output_artifact_references_for_stage,
};
pub(super) use janiform_market_time::calendar::ExchangeCalendarRegistry;

pub use contracts::{
    CalibrationStagedRunFailure, CalibrationStagedRunInputs, CalibrationStagedRunObserverEvent,
    CalibrationStagedRunReport, CalibrationStagedRunStageReport,
    calibration_checkpoint_reference_for_stage, calibration_last_success_checkpoint_reference,
};
pub use execution::{
    run_staged_calibration_with_checkpointing,
    run_staged_calibration_with_checkpointing_and_observer,
};
