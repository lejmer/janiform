use janiform_calibration_artifacts::{
    CalibrationEffortOptions, JointRefinementStageRestartEvaluationTimingDiagnostic,
};

use super::{
    CalibrationDiagnosticArtifactBundle, CalibrationDiagnosticArtifactReference,
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationStage,
    CalibrationStageCheckpointMetadata, CalibrationStageExecutionStatus,
    CalibrationStageSeedSource, HawkesStageCandidateInput, HawkesStageExecutionMetadata,
    HawkesStageExecutionReport, JointRefinementStageExecutionMetadata,
    JointRefinementStageExecutionReport, JointRefinementStageProgressPhase,
    LongVarianceStageExecutionMetadata, LongVarianceStageExecutionReport,
    MeanReversionStageExecutionInput, MeanReversionStageExecutionMetadata,
    MeanReversionStageExecutionReport, ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageExecutionMetadata, ReturnComponentsStageExecutionReport,
    ReturnComponentsStageProgressPhase, SeasonalityStageExecutionInput,
    SeasonalityStageExecutionMetadata, SeasonalityStageExecutionReport,
};
use crate::JointRefinementStageOptionPricingInput;
use crate::stages::joint_refinement::JointRefinementProgressCandidateAssessment;
use serde::{Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CalibrationStagedRunInputs {
    pub seasonality_stage_input: Option<SeasonalityStageExecutionInput>,
    pub hawkes_candidate_inputs: Option<Vec<HawkesStageCandidateInput>>,
    pub return_components_candidate_generation_input:
        Option<ReturnComponentsStageCandidateGenerationInput>,
    pub mean_reversion_stage_input: Option<MeanReversionStageExecutionInput>,
    pub joint_refinement_calibration_effort: Option<CalibrationEffortOptions>,
    pub joint_refinement_restart_candidate_budget_override: Option<usize>,
    pub treat_inadmissible_calibration_warning_as_error: bool,
    pub option_refinement_candidate_count_override: Option<usize>,
    pub option_refinement_wall_clock_budget_millis: Option<u64>,
    pub audit_option_pricing_enabled: bool,
    pub option_refinement_pricing_input: Option<JointRefinementStageOptionPricingInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "stage_report_kind",
    content = "report",
    rename_all = "snake_case"
)]
pub enum CalibrationStagedRunStageReport {
    Seasonality(Box<SeasonalityStageExecutionReport>),
    Hawkes(Box<HawkesStageExecutionReport>),
    ReturnComponents(Box<ReturnComponentsStageExecutionReport>),
    LongVariance(Box<LongVarianceStageExecutionReport>),
    MeanReversion(Box<MeanReversionStageExecutionReport>),
    JointRefinement(Box<JointRefinementStageExecutionReport>),
    OptionRefinement(Box<JointRefinementStageExecutionReport>),
    Finalization(Box<JointRefinementStageExecutionReport>),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CalibrationStageCheckpointComponents {
    pub(super) stage_output_schema_version: String,
    pub(super) run_global_seed: u64,
    pub(super) stage_seed: u64,
    pub(super) stage_seed_source: CalibrationStageSeedSource,
    pub(super) stage_seed_derivation_policy: &'static str,
    pub(super) stage_solver_settings: CalibrationOptimizerSettings,
}

trait CalibrationStageExecutionMetadataForCheckpoint {
    fn stage_output_schema_version(&self) -> &'static str;
    fn run_global_seed(&self) -> u64;
    fn stage_seed(&self) -> u64;
    fn stage_seed_source(&self) -> CalibrationStageSeedSource;
    fn stage_seed_derivation_policy(&self) -> &'static str;
    fn solver_settings(&self) -> &CalibrationOptimizerSettings;
}

impl CalibrationStageExecutionMetadataForCheckpoint for SeasonalityStageExecutionMetadata {
    fn stage_output_schema_version(&self) -> &'static str {
        self.stage_output_schema_version
    }

    fn run_global_seed(&self) -> u64 {
        self.run_global_seed
    }

    fn stage_seed(&self) -> u64 {
        self.stage_seed
    }

    fn stage_seed_source(&self) -> CalibrationStageSeedSource {
        self.stage_seed_source
    }

    fn stage_seed_derivation_policy(&self) -> &'static str {
        self.stage_seed_derivation_policy
    }

    fn solver_settings(&self) -> &CalibrationOptimizerSettings {
        &self.solver_settings
    }
}

impl CalibrationStageExecutionMetadataForCheckpoint for HawkesStageExecutionMetadata {
    fn stage_output_schema_version(&self) -> &'static str {
        self.stage_output_schema_version
    }

    fn run_global_seed(&self) -> u64 {
        self.run_global_seed
    }

    fn stage_seed(&self) -> u64 {
        self.stage_seed
    }

    fn stage_seed_source(&self) -> CalibrationStageSeedSource {
        self.stage_seed_source
    }

    fn stage_seed_derivation_policy(&self) -> &'static str {
        self.stage_seed_derivation_policy
    }

    fn solver_settings(&self) -> &CalibrationOptimizerSettings {
        &self.solver_settings
    }
}

impl CalibrationStageExecutionMetadataForCheckpoint for ReturnComponentsStageExecutionMetadata {
    fn stage_output_schema_version(&self) -> &'static str {
        self.stage_output_schema_version
    }

    fn run_global_seed(&self) -> u64 {
        self.run_global_seed
    }

    fn stage_seed(&self) -> u64 {
        self.stage_seed
    }

    fn stage_seed_source(&self) -> CalibrationStageSeedSource {
        self.stage_seed_source
    }

    fn stage_seed_derivation_policy(&self) -> &'static str {
        self.stage_seed_derivation_policy
    }

    fn solver_settings(&self) -> &CalibrationOptimizerSettings {
        &self.solver_settings
    }
}

impl CalibrationStageExecutionMetadataForCheckpoint for LongVarianceStageExecutionMetadata {
    fn stage_output_schema_version(&self) -> &'static str {
        self.stage_output_schema_version
    }

    fn run_global_seed(&self) -> u64 {
        self.run_global_seed
    }

    fn stage_seed(&self) -> u64 {
        self.stage_seed
    }

    fn stage_seed_source(&self) -> CalibrationStageSeedSource {
        self.stage_seed_source
    }

    fn stage_seed_derivation_policy(&self) -> &'static str {
        self.stage_seed_derivation_policy
    }

    fn solver_settings(&self) -> &CalibrationOptimizerSettings {
        &self.solver_settings
    }
}

impl CalibrationStageExecutionMetadataForCheckpoint for JointRefinementStageExecutionMetadata {
    fn stage_output_schema_version(&self) -> &'static str {
        self.stage_output_schema_version
    }

    fn run_global_seed(&self) -> u64 {
        self.run_global_seed
    }

    fn stage_seed(&self) -> u64 {
        self.stage_seed
    }

    fn stage_seed_source(&self) -> CalibrationStageSeedSource {
        self.stage_seed_source
    }

    fn stage_seed_derivation_policy(&self) -> &'static str {
        self.stage_seed_derivation_policy
    }

    fn solver_settings(&self) -> &CalibrationOptimizerSettings {
        &self.solver_settings
    }
}

impl CalibrationStageExecutionMetadataForCheckpoint for MeanReversionStageExecutionMetadata {
    fn stage_output_schema_version(&self) -> &'static str {
        self.stage_output_schema_version
    }

    fn run_global_seed(&self) -> u64 {
        self.run_global_seed
    }

    fn stage_seed(&self) -> u64 {
        self.stage_seed
    }

    fn stage_seed_source(&self) -> CalibrationStageSeedSource {
        self.stage_seed_source
    }

    fn stage_seed_derivation_policy(&self) -> &'static str {
        self.stage_seed_derivation_policy
    }

    fn solver_settings(&self) -> &CalibrationOptimizerSettings {
        &self.solver_settings
    }
}

fn checkpoint_components_from_metadata<M: CalibrationStageExecutionMetadataForCheckpoint>(
    metadata: Option<&M>,
) -> Option<CalibrationStageCheckpointComponents> {
    metadata.map(|metadata| CalibrationStageCheckpointComponents {
        stage_output_schema_version: metadata.stage_output_schema_version().to_string(),
        run_global_seed: metadata.run_global_seed(),
        stage_seed: metadata.stage_seed(),
        stage_seed_source: metadata.stage_seed_source(),
        stage_seed_derivation_policy: metadata.stage_seed_derivation_policy(),
        stage_solver_settings: metadata.solver_settings().clone(),
    })
}

impl CalibrationStagedRunStageReport {
    pub(super) const fn stage(&self) -> CalibrationStage {
        match self {
            Self::Seasonality(_) => CalibrationStage::Seasonality,
            Self::Hawkes(_) => CalibrationStage::Hawkes,
            Self::ReturnComponents(_) => CalibrationStage::ReturnComponents,
            Self::LongVariance(_) => CalibrationStage::LongVariance,
            Self::MeanReversion(_) => CalibrationStage::MeanReversion,
            Self::JointRefinement(_) => CalibrationStage::JointRefinement,
            Self::OptionRefinement(_) => CalibrationStage::OptionRefinement,
            Self::Finalization(_) => CalibrationStage::Finalization,
        }
    }

    pub const fn status(&self) -> CalibrationStageExecutionStatus {
        match self {
            Self::Seasonality(report) => report.status,
            Self::Hawkes(report) => report.status,
            Self::ReturnComponents(report) => report.status,
            Self::LongVariance(report) => report.status,
            Self::MeanReversion(report) => report.status,
            Self::JointRefinement(report) => report.status,
            Self::OptionRefinement(report) => report.status,
            Self::Finalization(report) => report.status,
        }
    }

    pub(super) fn checkpoint_components(&self) -> Option<CalibrationStageCheckpointComponents> {
        match self {
            Self::Seasonality(report) => {
                checkpoint_components_from_metadata(report.metadata.as_ref())
            }
            Self::Hawkes(report) => checkpoint_components_from_metadata(report.metadata.as_ref()),
            Self::ReturnComponents(report) => {
                checkpoint_components_from_metadata(report.metadata.as_ref())
            }
            Self::LongVariance(report) => {
                checkpoint_components_from_metadata(report.metadata.as_ref())
            }
            Self::MeanReversion(report) => {
                checkpoint_components_from_metadata(report.metadata.as_ref())
            }
            Self::JointRefinement(report) => {
                checkpoint_components_from_metadata(report.metadata.as_ref())
            }
            Self::OptionRefinement(report) => {
                checkpoint_components_from_metadata(report.metadata.as_ref())
            }
            Self::Finalization(report) => {
                checkpoint_components_from_metadata(report.metadata.as_ref())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum CalibrationStagedRunFailure {
    InvalidRunContract {
        error_message: String,
    },
    MissingMemoryCheckpointStore,
    MissingDiskCheckpointDirectory,
    MissingResumeCheckpointForRun {
        run_id: String,
    },
    DatasetSnapshotPersistFailed {
        artifact_reference: String,
        error_message: String,
    },
    StageParameterArtifactLoadFailed {
        stage: CalibrationStage,
        artifact_reference: String,
        error_message: String,
    },
    StageParameterArtifactBuildFailed {
        stage: CalibrationStage,
        artifact_reference: String,
        error_message: String,
    },
    StageParameterArtifactPersistFailed {
        stage: CalibrationStage,
        artifact_reference: String,
        error_message: String,
    },
    ArtifactManifestPersistFailed {
        error_message: String,
    },
    MissingStageInput {
        stage: CalibrationStage,
    },
    MissingInMemoryUpstreamPayload {
        required_stage: CalibrationStage,
        consuming_stage: CalibrationStage,
    },
    InvalidCheckpointReference {
        checkpoint_reference: String,
        error_message: String,
    },
    CheckpointLoadFailed {
        checkpoint_reference: String,
        error_message: String,
    },
    CheckpointCompatibilityFailed {
        checkpoint_reference: String,
        error_message: String,
    },
    CheckpointPersistFailed {
        checkpoint_reference: String,
        error_message: String,
    },
    MissingCheckpointMetadataForSuccessfulStage {
        stage: CalibrationStage,
    },
    StageExecutionFailed {
        stage: CalibrationStage,
        #[serde(skip_serializing_if = "Option::is_none")]
        error_message: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationStagedRunReport {
    pub schema_version: &'static str,
    pub failure_triage_policy: &'static str,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub status: CalibrationStageExecutionStatus,
    pub stage_reports: Vec<CalibrationStagedRunStageReport>,
    pub emitted_checkpoints: Vec<CalibrationStageCheckpointMetadata>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostic_artifact_references: Vec<CalibrationDiagnosticArtifactReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic_artifact_bundle: Option<CalibrationDiagnosticArtifactBundle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed_from_checkpoint_reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed_from_last_successful_stage: Option<CalibrationStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_from_stage: Option<CalibrationStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<CalibrationStagedRunFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationStageProgressPhase {
    ReturnComponents(ReturnComponentsStageProgressPhase),
    JointRefinement(JointRefinementStageProgressPhase),
}

impl From<ReturnComponentsStageProgressPhase> for CalibrationStageProgressPhase {
    fn from(phase: ReturnComponentsStageProgressPhase) -> Self {
        Self::ReturnComponents(phase)
    }
}

impl From<JointRefinementStageProgressPhase> for CalibrationStageProgressPhase {
    fn from(phase: JointRefinementStageProgressPhase) -> Self {
        Self::JointRefinement(phase)
    }
}

impl Serialize for CalibrationStageProgressPhase {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::ReturnComponents(phase) => phase.serialize(serializer),
            Self::JointRefinement(phase) => phase.serialize(serializer),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event_kind", rename_all = "snake_case")]
pub enum CalibrationStagedRunObserverEvent {
    RunStarted {
        run_id: String,
        execution_mode: CalibrationExecutionMode,
        stage_count: usize,
        stage_sequence: Vec<CalibrationStage>,
        completed_stage_count: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        resumed_from_checkpoint_reference: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        resumed_from_last_successful_stage: Option<CalibrationStage>,
        #[serde(skip_serializing_if = "Option::is_none")]
        started_from_stage: Option<CalibrationStage>,
    },
    StageStarted {
        run_id: String,
        stage: CalibrationStage,
        stage_index: usize,
        stage_count: usize,
    },
    StageCompleted {
        run_id: String,
        stage: CalibrationStage,
        stage_index: usize,
        stage_count: usize,
        runtime_seconds: f64,
        stage_report: Box<CalibrationStagedRunStageReport>,
    },
    StageProgress(Box<CalibrationStagedRunStageProgressEvent>),
    RunCompleted {
        run_id: String,
        execution_mode: CalibrationExecutionMode,
        total_runtime_seconds: f64,
        stage_report_count: usize,
    },
    RunFailed {
        run_id: String,
        execution_mode: CalibrationExecutionMode,
        total_runtime_seconds: f64,
        completed_stage_count: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        failed_stage: Option<CalibrationStage>,
        failure: CalibrationStagedRunFailure,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationStagedRunStageProgressEvent {
    pub run_id: String,
    pub stage: CalibrationStage,
    pub stage_index: usize,
    pub stage_count: usize,
    pub progress_phase: CalibrationStageProgressPhase,
    pub completed_units: usize,
    pub total_units: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_progress_percent: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate_progress_percent: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_item_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_restart_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_work_item_status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub planned_work_item_labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_restart_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_outer_cycles: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_outer_cycles: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_evaluation_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_evaluation_budget: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluation_progress_percent: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_pricing_interval_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pricing_interval_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_pricing_row_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pricing_row_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_completed_units: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cumulative_local_evaluation_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_global_loss: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_restart_best_global_loss: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_candidate_assessment: Option<JointRefinementProgressCandidateAssessment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_restart_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_global_loss: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_coarse_consistency_fit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_coarse_consistency_fit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_closed_form_buy_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_closed_form_sell_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_tilted_mixed_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_evaluation_timing_diagnostic:
        Option<JointRefinementStageRestartEvaluationTimingDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_convergence_ratio: Option<f64>,
}
