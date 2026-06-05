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
    HawkesStageExecutionReport, HawkesStageFittedPayload, HawkesStageSeasonalityInput,
    run_hawkes_calibration_stage,
};
pub(super) use crate::stages::joint_refinement::{
    FinalizationCalibrationStageEntrypointInput, JointRefinementCalibrationStageEntrypointInput,
    JointRefinementStageExecutionInput, JointRefinementStageExecutionMetadata,
    JointRefinementStageExecutionReport, JointRefinementStageOptionPricingInput,
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
    JointRefinementStageRuntimeHandoff, JointRefinementStageSeasonalityFactorsInput,
    OptionRefinementCalibrationStageEntrypointInput,
    run_finalization_calibration_stage_with_progress_observer_and_persistence,
    run_joint_refinement_calibration_stage_with_progress_observer_and_persistence,
    run_option_refinement_calibration_stage_with_progress_observer_and_persistence,
};
pub(super) use crate::stages::long_variance::{
    LongVarianceStageExecutionInput, LongVarianceStageExecutionMetadata,
    LongVarianceStageExecutionReport, LongVarianceStageProgressPhase,
    LongVarianceStageProgressUpdate, run_long_variance_calibration_stage,
    run_long_variance_calibration_stage_with_progress_observer,
};
pub(super) use crate::stages::mean_reversion::{
    MeanReversionStageExecutionInput, MeanReversionStageExecutionMetadata,
    MeanReversionStageExecutionReport, MeanReversionStageProgressPhase,
    MeanReversionStageProgressUpdate, run_mean_reversion_calibration_stage,
    run_mean_reversion_calibration_stage_with_progress_observer,
};
pub(super) use crate::stages::news_shocks::{
    NewsShocksStageExecutionInput, NewsShocksStageExecutionMetadata,
    NewsShocksStageExecutionReport, run_news_shocks_calibration_stage,
};
pub(super) use crate::stages::return_backfit::{
    ReturnBackfitStageExecutionInput, ReturnBackfitStageExecutionMetadata,
    ReturnBackfitStageExecutionReport, run_return_backfit_calibration_stage,
};
pub(super) use crate::stages::return_components::{
    ReturnComponentsStageCandidateGenerationInput, ReturnComponentsStageExecutionInput,
    ReturnComponentsStageExecutionMetadata, ReturnComponentsStageExecutionReport,
    ReturnComponentsStageProgressPhase, ReturnComponentsStageProgressUpdate,
    run_return_components_calibration_stage,
    run_return_components_calibration_stage_with_progress_observer,
};
pub(super) use crate::stages::seasonality::{
    CalibrationStageExecutionStatus, SeasonalityStageExecutionInput,
    SeasonalityStageExecutionMetadata, SeasonalityStageExecutionReport,
    run_seasonality_calibration_stage,
};
pub(super) use janiform_calibration_artifacts::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY,
    CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_LONG_VARIANCE_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_NEWS_SHOCKS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY, CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION,
    CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY, CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationRunContract,
    CalibrationRunSpec, CalibrationStage, CalibrationStageCheckpointMetadata,
    CalibrationStageHandoffTransport, CalibrationStageParameterArtifact,
    CalibrationStageParameterArtifactInput, CalibrationStageParameterBuildMetadata,
    CalibrationStageParameterOptimizerStateInput, CalibrationStageParameterPayload,
    CalibrationStagePayloadSource, CalibrationStageSeedSource,
    CalibrationStageSelectedCandidateSource, calibration_stage_artifact_reference,
    stage_output_artifact_references_for_stage,
};
pub(super) use janiform_market_time::calendar::ExchangeCalendarRegistry;

pub use contracts::{
    CalibrationStageProgressPhase, CalibrationStagedRunFailure, CalibrationStagedRunInputs,
    CalibrationStagedRunObserverEvent, CalibrationStagedRunReport,
    CalibrationStagedRunStageProgressEvent, CalibrationStagedRunStageReport,
};
pub use execution::{
    run_staged_calibration_with_checkpointing,
    run_staged_calibration_with_checkpointing_and_observer,
};
