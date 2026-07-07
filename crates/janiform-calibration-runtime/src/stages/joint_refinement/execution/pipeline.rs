mod baseline;
mod candidate_search;
mod candidate_selection;
mod final_assembly;
mod infrastructure;
mod market_pressure;
mod option_refinement;
mod preparation;
mod progress;
mod reporting;
mod types;
mod workflow;

pub(crate) use baseline::ensure_valid_selected_candidate_outcome;
pub(in crate::stages::joint_refinement::execution) use candidate_search::count_valid_candidate_outcomes;
#[cfg(test)]
pub(crate) use candidate_selection::option_refinement_candidate_frontier_from_selected_outcomes;
pub(in crate::stages::joint_refinement::execution) use candidate_selection::{
    build_selectable_candidate_outcomes, count_admissible_candidate_outcomes,
};
pub(in crate::stages::joint_refinement::execution) use option_refinement::require_option_context_fingerprint;
pub(in crate::stages::joint_refinement::execution) use types::{
    BaselineEvaluationArtifacts, CandidateSearchArtifacts,
};
pub(crate) use workflow::{
    FinalizationPipelineStageRunInput, OptionRefinementPipelineStageRunInput,
};
pub(super) use workflow::{
    run_finalization_calibration_stage_internal, run_joint_refinement_calibration_stage_internal,
    run_option_refinement_calibration_stage_internal,
};
