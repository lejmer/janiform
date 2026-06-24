use std::collections::VecDeque;

use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, JointRefinementStageCandidateEvaluationTimingDiagnostic,
};

use crate::stages::joint_refinement::contracts::JointRefinementStageProgressUpdate;

use super::super::super::{JointRefinementCandidateOutcome, JointRefinementContext};
use super::super::workflow::CandidateObjectiveEvaluation;
use super::state::PopulationControlState;

#[derive(Debug, Clone)]
pub(super) struct PopulationControlFitSnapshot {
    pub(super) candidate_id: String,
    pub(super) objective: f64,
    pub(super) exact_likelihood_evaluator_counts:
        Option<crate::stages::return_components::ContinuousLikelihoodEvaluatorCounts>,
}

#[derive(Debug, Clone)]
pub(super) struct PopulationControlCandidateSpec {
    pub(super) candidate_position: usize,
    pub(super) candidate_id: String,
    pub(super) parameter_vector: Vec<f64>,
    pub(super) schedule_category: &'static str,
    pub(super) search_direction_position: Option<usize>,
    pub(super) search_direction_label: String,
}

#[derive(Debug)]
pub(super) struct PopulationControlSearchJob {
    pub(super) candidate_position: usize,
    pub(super) candidate_id: String,
    pub(super) parameter_vector: Vec<f64>,
    pub(super) candidate_evaluation_count: usize,
    pub(super) schedule_category: &'static str,
    pub(super) search_direction_position: Option<usize>,
    pub(super) search_direction_label: String,
}

#[derive(Debug)]
pub(super) struct PopulationControlSearchResult {
    pub(super) candidate_position: usize,
    pub(super) candidate_id: String,
    pub(super) parameter_vector: Vec<f64>,
    pub(super) candidate_evaluation_count: usize,
    pub(super) schedule_category: &'static str,
    pub(super) search_direction_position: Option<usize>,
    pub(super) search_direction_label: String,
    pub(super) elapsed_millis: u64,
    pub(super) evaluation: Option<CandidateObjectiveEvaluation>,
}

#[derive(Debug)]
pub(super) struct PopulationControlFinalizationJob {
    pub(super) candidate_position: usize,
    pub(super) candidate_id: String,
    pub(super) seed_vector: Vec<f64>,
    pub(super) candidate_vector: Vec<f64>,
    pub(super) evaluation_count: usize,
    pub(super) stop_reason: String,
}

#[derive(Debug)]
pub(super) struct PopulationControlFinalizationResult {
    pub(super) candidate_position: usize,
    pub(super) elapsed_millis: u64,
    pub(super) outcome: JointRefinementCandidateOutcome,
}

pub(super) enum PopulationControlPoolJob {
    Search(PopulationControlSearchJob),
    Finalization(PopulationControlFinalizationJob),
}

pub(super) enum PopulationControlPoolResult {
    Search(Box<PopulationControlSearchResult>),
    Finalization(Box<PopulationControlFinalizationResult>),
}

pub(super) struct PopulationControlWorkerPoolInput<'a> {
    pub(super) state: &'a mut PopulationControlState,
    pub(super) context: &'a JointRefinementContext,
    pub(super) solver_settings: &'a CalibrationOptimizerSettings,
    pub(super) search_budget: usize,
    pub(super) total_progress_units: usize,
    pub(super) progress_sender:
        Option<&'a std::sync::mpsc::Sender<JointRefinementStageProgressUpdate>>,
    pub(super) evaluation_timing_diagnostics:
        &'a mut Vec<JointRefinementStageCandidateEvaluationTimingDiagnostic>,
}

pub(super) struct PopulationControlWorkerPoolState<'a> {
    pub(super) state: &'a mut PopulationControlState,
    pub(super) worker_capacity: usize,
    pub(super) pending_search_jobs: VecDeque<PopulationControlCandidateSpec>,
    pub(super) candidate_outcomes: Vec<Option<JointRefinementCandidateOutcome>>,
    pub(super) next_finalization_candidate_position: usize,
    pub(super) finalization_scheduled: Vec<bool>,
    pub(super) evaluation_timing_diagnostics:
        &'a mut Vec<JointRefinementStageCandidateEvaluationTimingDiagnostic>,
}
