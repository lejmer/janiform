mod coarse_consistency;
mod entrypoint;
mod inputs;
mod optimization;
mod option_refinement;
mod pipeline;
mod pricing_context;
mod report;
mod selection;
mod shared;
mod state;

#[cfg(test)]
mod tests;

pub use entrypoint::run_joint_refinement_calibration_stage;
pub use state::JointRefinementSeedSummary;

pub(crate) use entrypoint::run_joint_refinement_calibration_stage_with_progress_observer;

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
    desired_joint_refinement_restart_count, joint_refinement_budget_schedule,
    optimize_restart_seeds,
};
pub(crate) use option_refinement::{
    desired_joint_refinement_option_refinement_candidate_count, run_option_refinement,
};
pub(crate) use pricing_context::{
    build_option_calibration_diagnostics, build_option_pricing_context,
    resolve_option_surface_tier_pricing_artifact,
};
pub(crate) use report::{
    baseline_buckets_from_evaluation, build_joint_finalist_seed_summaries,
    build_joint_refinement_stage_failure_report, build_joint_restart_diagnostics,
    build_joint_simulation_summaries, option_pricing_execution_failure_message,
    simulation_infrastructure_failure_message,
};
#[cfg(test)]
pub(crate) use selection::{
    JointRefinementAdmissibilityThresholds, admissibility_threshold_from_sample_count,
    joint_breakdown_is_admissible,
};
pub(crate) use selection::{
    build_joint_candidate_frontier_diagnostics,
    select_restart_outcomes_with_lazy_coarse_consistency,
};
pub(crate) use shared::{
    build_exchange_calendar_registry_artifact, build_halton_restart_id, clamp_in_place,
    deduplicate_restart_seed_candidates, halton_parameter_vector, ordered_parameter_bits,
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
    JointRefinementProfilingState, JointRefinementRestartOutcome,
    JointRefinementSeedSummaryCacheKey,
};
