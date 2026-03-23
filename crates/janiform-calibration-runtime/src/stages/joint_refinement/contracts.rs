use serde::Serialize;

use crate::optimizer_adapter::CalibrationOptimizerAdapterRunMetadata;
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::return_components::{
    ReturnComponentsStageCandidateGenerationInput, ReturnComponentsStageFittedPayload,
};
use crate::stages::seasonality::CalibrationStageExecutionStatus;
use janiform_calibration_artifacts::{
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    JointRefinementStageFittedParameters, JointRefinementStageObjectiveBreakdown,
    JointRefinementStageOptimizerDiagnostics,
};

#[derive(Debug, Clone, PartialEq)]
pub enum JointRefinementStageHawkesInput {
    InMemoryOutput {
        payload: HawkesStageFittedPayload,
    },
    PersistedArtifactReference {
        artifact_reference: String,
        payload: Option<HawkesStageFittedPayload>,
    },
}

impl JointRefinementStageHawkesInput {
    pub const fn transport(&self) -> CalibrationStageHandoffTransport {
        match self {
            Self::InMemoryOutput { .. } => CalibrationStageHandoffTransport::InMemoryOutput,
            Self::PersistedArtifactReference { .. } => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        }
    }

    pub fn artifact_reference(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { .. } => None,
            Self::PersistedArtifactReference {
                artifact_reference, ..
            } => Some(artifact_reference.as_str()),
        }
    }

    pub fn payload(&self) -> Option<&HawkesStageFittedPayload> {
        match self {
            Self::InMemoryOutput { payload } => Some(payload),
            Self::PersistedArtifactReference { payload, .. } => payload.as_ref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum JointRefinementStageReturnComponentsInput {
    InMemoryOutput {
        payload: ReturnComponentsStageFittedPayload,
    },
    PersistedArtifactReference {
        artifact_reference: String,
        payload: Option<ReturnComponentsStageFittedPayload>,
    },
}

impl JointRefinementStageReturnComponentsInput {
    pub const fn transport(&self) -> CalibrationStageHandoffTransport {
        match self {
            Self::InMemoryOutput { .. } => CalibrationStageHandoffTransport::InMemoryOutput,
            Self::PersistedArtifactReference { .. } => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        }
    }

    pub fn artifact_reference(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { .. } => None,
            Self::PersistedArtifactReference {
                artifact_reference, ..
            } => Some(artifact_reference.as_str()),
        }
    }

    pub fn payload(&self) -> Option<&ReturnComponentsStageFittedPayload> {
        match self {
            Self::InMemoryOutput { payload } => Some(payload),
            Self::PersistedArtifactReference { payload, .. } => payload.as_ref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JointRefinementStageSeasonalityFactorsInput {
    pub intraday_minutes_per_bin: u16,
    pub intraday_factor_by_bin: Vec<f64>,
    pub day_of_week_factor: Vec<f64>,
    pub day_of_year_factor: Vec<f64>,
    pub timestamp_utc_nanos: Vec<i64>,
    pub normalized_combined_factor_by_event: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JointRefinementStageExecutionInput {
    pub seasonality_factors_input: JointRefinementStageSeasonalityFactorsInput,
    pub hawkes_input: JointRefinementStageHawkesInput,
    pub return_components_input: JointRefinementStageReturnComponentsInput,
    pub candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageFittedPayload {
    pub schema_version: &'static str,
    pub selected_candidate_id: String,
    pub fitted_parameters: JointRefinementStageFittedParameters,
    pub baseline_hawkes_fitted_parameters:
        janiform_calibration_artifacts::HawkesStageFittedParameters,
    pub baseline_return_components_fitted_parameters:
        janiform_calibration_artifacts::ReturnComponentsStageFittedParameters,
    pub objective_breakdown: JointRefinementStageObjectiveBreakdown,
    pub optimizer_diagnostics: JointRefinementStageOptimizerDiagnostics,
    pub candidate_frontier: Vec<JointRefinementStageCandidateDiagnostic>,
    pub simulation_summaries: Vec<JointRefinementStageSimulationSummary>,
    pub coarse_tick_consistency: Vec<JointRefinementStageCoarseTickConsistencyMetric>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageCandidateDiagnostic {
    pub candidate_id: String,
    pub valid: bool,
    pub admissible: bool,
    pub pareto_rank: usize,
    pub lexicographic_rank: usize,
    pub scalar_tie_break: f64,
    pub local_evaluation_count: usize,
    pub stop_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_reason: Option<String>,
    pub objective_terms: Vec<JointRefinementStageObjectiveTermDiagnostic>,
    pub objective_breakdown: JointRefinementStageObjectiveBreakdown,
    pub canonical_coordinates: Vec<(String, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageObjectiveTermDiagnostic {
    pub metric_id: String,
    pub target_id: String,
    pub target_family: String,
    pub observed_value: f64,
    pub simulated_value: f64,
    pub uncertainty_scale: f64,
    pub normalized_error: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageSimulationSummary {
    pub simulation_seed: u64,
    pub horizon_label: String,
    pub event_count: usize,
    pub realized_variance: f64,
    pub mean_absolute_return: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageCoarseTickConsistencyMetric {
    pub simulation_seed: u64,
    pub horizon_label: String,
    pub metric_id: String,
    pub coarse_value: f64,
    pub tick_value: f64,
    pub absolute_delta: f64,
    pub normalized_delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
    pub optimizer_adapter_metadata: CalibrationOptimizerAdapterRunMetadata,
    pub input_candidate_count: usize,
    pub valid_candidate_count: usize,
    pub selected_candidate_raw_objective: f64,
    pub selected_candidate_penalized_objective: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum JointRefinementStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingSeasonalityPayload,
    MissingHawkesPayload,
    MissingReturnComponentsPayload,
    InvalidCandidateGenerationInput { error_message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<JointRefinementStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JointRefinementStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<JointRefinementStageExecutionFailure>,
}
