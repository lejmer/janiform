use std::collections::BTreeMap;
use std::sync::Arc;

use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::mean_reversion::MeanReversionStageFittedPayload;
use crate::stages::return_components::{
    ReturnComponentsStageFittedPayload,
    ReturnComponentsStageJointRefinementObservedSignalPreparation,
};
use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, CalibrationResolvedStageSeed, CalibrationStage,
};

use super::super::{
    JointRefinementActiveParameterLayout, JointRefinementCandidateOutcome, JointRefinementContext,
    JointRefinementResumeStore,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stages::joint_refinement::execution) enum JointRefinementPipelineStage {
    JointRefinement,
    OptionRefinement,
    Finalization,
}

impl JointRefinementPipelineStage {
    pub(super) const fn calibration_stage(self) -> CalibrationStage {
        match self {
            Self::JointRefinement => CalibrationStage::JointRefinement,
            Self::OptionRefinement => CalibrationStage::OptionRefinement,
            Self::Finalization => CalibrationStage::Finalization,
        }
    }
}

pub(super) struct ResolvedStagePayloads<'a> {
    pub(super) stage_solver_settings: CalibrationOptimizerSettings,
    pub(super) hawkes_payload: &'a HawkesStageFittedPayload,
    pub(super) return_components_payload: &'a ReturnComponentsStageFittedPayload,
    pub(super) mean_reversion_payload: &'a MeanReversionStageFittedPayload,
    pub(super) resolved_stage_seed: CalibrationResolvedStageSeed,
    pub(super) observed_signal_preparation:
        ReturnComponentsStageJointRefinementObservedSignalPreparation,
    pub(super) baseline_modeled_open_event_rate_per_hour: f64,
    pub(super) final_simulation_seeds: Vec<u64>,
}

pub(super) struct InitialPipelineState<'a> {
    pub(super) resolved_stage_payloads: ResolvedStagePayloads<'a>,
    pub(super) resume_store: JointRefinementResumeStore,
}

pub(super) struct PreparedBootstrapContext {
    pub(super) bootstrap_context: JointRefinementContext,
    pub(super) baseline_parameter_vector: Vec<f64>,
    pub(super) baseline_15m_calibration_sample:
        crate::stages::return_components::ReturnComponentsStageContinuousReturnCalibrationSample,
}

pub(super) struct PreparedObservedInputs {
    pub(super) observed_data: super::super::JointRefinementObservedData,
    pub(super) baseline_15m_calibration_sample:
        crate::stages::return_components::ReturnComponentsStageContinuousReturnCalibrationSample,
}

pub(super) struct PreparedParameterization {
    pub(super) active_parameter_layout: JointRefinementActiveParameterLayout,
    pub(super) baseline_parameter_vector: Vec<f64>,
    pub(super) lower_bounds: Vec<f64>,
    pub(super) upper_bounds: Vec<f64>,
    pub(super) option_pricing_refinement_enabled: bool,
    pub(super) option_pricing_context:
        Option<super::super::state::JointRefinementOptionPricingContext>,
}

pub(in crate::stages::joint_refinement::execution) struct BaselineEvaluationArtifacts {
    pub(in crate::stages::joint_refinement::execution) baseline_candidate_outcome:
        JointRefinementCandidateOutcome,
    pub(super) baseline_coarse_consistency_artifacts:
        Option<super::super::coarse_consistency::JointRefinementCoarseConsistencyArtifacts>,
}

pub(in crate::stages::joint_refinement::execution) struct CandidateSearchArtifacts {
    pub(in crate::stages::joint_refinement::execution) candidate_outcomes: Vec<JointRefinementCandidateOutcome>,
    pub(in crate::stages::joint_refinement::execution) candidate_evaluation_timing_diagnostics:
        Vec<janiform_calibration_artifacts::JointRefinementStageCandidateEvaluationTimingDiagnostic>,
    pub(in crate::stages::joint_refinement::execution) valid_candidate_count: usize,
    pub(in crate::stages::joint_refinement::execution) best_candidate: JointRefinementCandidateOutcome,
    pub(in crate::stages::joint_refinement::execution) option_refinement_candidate_frontier: Vec<JointRefinementCandidateOutcome>,
    pub(in crate::stages::joint_refinement::execution) selected_option_search_tier_pricing_artifact:
        Option<Arc<super::super::state::JointRefinementOptionTierPricingArtifact>>,
    pub(in crate::stages::joint_refinement::execution) selected_option_selection_tier_pricing_artifact:
        Option<Arc<super::super::state::JointRefinementOptionTierPricingArtifact>>,
    pub(in crate::stages::joint_refinement::execution) selected_option_audit_tier_pricing_artifact:
        Option<Arc<super::super::state::JointRefinementOptionTierPricingArtifact>>,
    pub(in crate::stages::joint_refinement::execution) option_search_selection_agreement: Option<
        janiform_calibration_artifacts::JointRefinementStageOptionSearchSelectionAgreementDiagnostics,
    >,
    pub(in crate::stages::joint_refinement::execution) coarse_consistency_artifacts_by_candidate_id:
        BTreeMap<String, super::super::coarse_consistency::JointRefinementCoarseConsistencyArtifacts>,
    pub(in crate::stages::joint_refinement::execution) warning: Option<String>,
    pub(in crate::stages::joint_refinement::execution) population_refinement_wall_clock_millis: u64,
    pub(in crate::stages::joint_refinement::execution) coarse_consistency_wall_clock_millis: u64,
}

pub(super) struct CandidateSearchPlan {
    pub(super) candidate_seeds: Vec<(String, Vec<f64>)>,
    pub(super) budget_schedule: super::super::optimization::JointRefinementBudgetSchedule,
}

pub(super) struct CandidateSelectionArtifacts {
    pub(super) best_candidate: JointRefinementCandidateOutcome,
    pub(super) option_refinement_candidate_frontier: Vec<JointRefinementCandidateOutcome>,
    pub(super) coarse_consistency_artifacts_by_candidate_id: BTreeMap<
        String,
        super::super::coarse_consistency::JointRefinementCoarseConsistencyArtifacts,
    >,
    pub(super) warning: Option<String>,
    pub(super) coarse_consistency_wall_clock_millis: u64,
}

pub(super) struct FinalAssemblyArtifacts {
    pub(super) candidate_frontier:
        Vec<crate::stages::joint_refinement::contracts::JointRefinementStageCandidateDiagnostic>,
    pub(super) simulation_summaries:
        Vec<crate::stages::joint_refinement::contracts::JointRefinementStageSimulationSummary>,
    pub(super) coarse_tick_consistency: Vec<
        crate::stages::joint_refinement::contracts::JointRefinementStageCoarseTickConsistencyMetric,
    >,
    pub(super) option_calibration_diagnostics:
        Option<janiform_calibration_artifacts::JointRefinementStageOptionCalibrationDiagnostics>,
    pub(super) final_seed_evaluation_wall_clock_millis: u64,
    pub(super) final_assembly_wall_clock_millis: u64,
}
