mod coarse_consistency;
mod entrypoint;
mod inputs;
mod optimization;
mod option_objective_gradient;
mod option_reduced_model;
mod option_refinement;
mod pipeline;
mod pricing_context;
mod report;
mod resume_state;
mod selection;
mod shared;
mod state;

#[cfg(test)]
mod tests;

pub use entrypoint::run_joint_refinement_calibration_stage;
pub use state::JointRefinementSeedSummary;

pub(crate) use entrypoint::run_joint_refinement_calibration_stage_with_progress_observer_and_persistence;

#[cfg(test)]
pub(crate) use coarse_consistency::extend_joint_coarse_tick_consistency_metrics_for_seed;
pub(crate) use coarse_consistency::{
    build_joint_candidate_coarse_consistency_artifacts, build_joint_coarse_tick_consistency_metrics,
};
#[cfg(test)]
pub(crate) use inputs::sampled_observed_tick_returns;
pub(crate) use inputs::{build_observed_data, build_summary_simulation_session_context};
#[cfg(test)]
pub(crate) use optimization::joint_refinement_restart_block_powell_settings;
pub(crate) use optimization::{
    desired_joint_refinement_restart_candidate_budget, joint_refinement_budget_schedule,
    optimize_restart_seeds, precheck_seed_summary_workload,
};
pub(crate) use option_refinement::{
    desired_joint_refinement_option_refinement_candidate_count, run_option_refinement,
};
#[cfg(test)]
pub(crate) use pipeline::ensure_valid_selected_restart_outcome;
pub(crate) use pricing_context::{
    OptionPricingProgressInput, OptionSurfaceTierPricingArtifactResolutionInput,
    build_option_calibration_diagnostics, build_option_pricing_context,
    resolve_option_surface_tier_pricing_artifact,
};
pub(crate) use report::{
    baseline_buckets_from_evaluation, build_joint_finalist_seed_summaries,
    build_joint_refinement_stage_failure_report, build_joint_restart_diagnostics,
    build_joint_simulation_summaries, option_pricing_execution_failure_message,
    simulation_infrastructure_failure_message,
};
pub(crate) use resume_state::{JointRefinementResumeStore, PersistedRestartSelectionArtifacts};
pub(crate) use selection::{
    JOINT_REFINEMENT_THRESHOLD_RELATIVE_ADMISSIBILITY_SCORE,
    JointRefinementAdmissibilityThresholds, build_joint_candidate_frontier_diagnostics,
    joint_raw_metrics_are_dynamics_feasible, joint_refinement_admissibility_thresholds,
    joint_restart_outcome_is_admissible, select_restart_outcomes_with_lazy_coarse_consistency,
};
#[cfg(test)]
pub(crate) use selection::{
    admissibility_threshold_from_sample_count, joint_raw_metrics_are_admissible,
};
pub(crate) use shared::{
    build_exchange_calendar_registry_artifact, build_halton_restart_id, clamp_in_place,
    deduplicate_restart_seed_candidates, halton_parameter_vector,
    joint_refinement_has_parallel_worker_capacity, joint_refinement_parallel_worker_count,
    ordered_parameter_bits, with_joint_refinement_thread_pool,
};
pub(crate) use state::{
    JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND, JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR,
    JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT, JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    JointRefinementActiveParameterLayout, JointRefinementBaselineBuckets,
    JointRefinementCandidateEvaluation, JointRefinementCoarseConsistencySummary,
    JointRefinementContext, JointRefinementDeterministicCacheKey,
    JointRefinementDeterministicEvaluation, JointRefinementFidelityTier,
    JointRefinementObjectiveCacheKey, JointRefinementObservedData,
    JointRefinementOptionPricingCacheKey, JointRefinementOptionTierPricingArtifact,
    JointRefinementProfilingState, JointRefinementRawAdmissibilityMetrics,
    JointRefinementRestartOutcome, JointRefinementSeedSummaryCacheKey,
};
