mod coarse_consistency;
mod entrypoint;
mod inputs;
mod optimization;
mod option_refinement;
mod pipeline;
mod pricing_context;
mod report;
mod resume_state;
mod selection;
mod shared;
mod stage_resume;
mod state;
#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;

pub use entrypoint::run_joint_refinement_calibration_stage;
pub use state::JointRefinementSeedSummary;

pub(crate) use entrypoint::{
    FinalizationCalibrationStageEntrypointInput, JointRefinementCalibrationStageEntrypointInput,
    OptionRefinementCalibrationStageEntrypointInput,
    run_finalization_calibration_stage_with_progress_observer_and_persistence,
    run_joint_refinement_calibration_stage_with_progress_observer_and_persistence,
    run_option_refinement_calibration_stage_with_progress_observer_and_persistence,
};

#[cfg(test)]
pub(crate) use coarse_consistency::extend_joint_coarse_tick_consistency_metrics_for_seed;
pub(crate) use coarse_consistency::{
    build_joint_candidate_coarse_consistency_artifacts_result,
    build_joint_coarse_interval_observations, build_joint_coarse_tick_consistency_metrics,
};
pub(crate) use inputs::{
    JointRefinementObservedDataBuildInput, build_observed_data,
    build_summary_simulation_session_context,
    rescaled_observed_tick_calibration_sample_for_modeled_rate,
};
#[cfg(test)]
pub(crate) use inputs::{sampled_observed_tick_returns, tick_shape_sample_target_count};
pub(crate) use optimization::{
    desired_joint_refinement_candidate_budget, joint_refinement_budget_schedule,
    optimize_candidate_seeds, precheck_seed_summary_workload,
};
pub(crate) use option_refinement::{
    OPTION_PRICING_BLOCK_SEARCH_COORDINATE_COUNT,
    desired_joint_refinement_option_refinement_candidate_count, run_option_refinement,
};
#[cfg(test)]
pub(crate) use pipeline::{
    ensure_valid_selected_candidate_outcome,
    option_refinement_candidate_frontier_from_selected_outcomes,
};
pub(crate) use pricing_context::{
    OptionPricingProgressInput, OptionPricingProgressSnapshot, OptionPricingProgressSnapshotSource,
    OptionSurfaceTierPricingArtifactResolutionInput, build_option_calibration_diagnostics,
    build_option_pricing_context, resolve_option_surface_tier_pricing_artifact,
};
pub(crate) use report::{
    build_joint_candidate_diagnostics, build_joint_finalist_seed_summaries,
    build_joint_refinement_like_stage_failure_report_with_runtime_profile,
    build_joint_refinement_stage_failure_report, build_joint_simulation_summaries,
    option_pricing_execution_failure_message, simulation_infrastructure_failure_message,
};
pub(crate) use resume_state::{
    JointRefinementResumeStore, PersistedCandidateSelectionArtifacts,
    option_pricing_context_fingerprint,
};
#[cfg(test)]
pub(crate) use selection::admissibility_threshold_from_sample_count;
pub(crate) use selection::{
    build_joint_candidate_frontier_diagnostics, joint_candidate_outcome_is_admissible,
    select_candidate_outcomes_with_lazy_coarse_consistency,
};
pub(crate) use shared::{
    build_exchange_calendar_registry_artifact, build_halton_candidate_id,
    deduplicate_candidate_seeds, elapsed_millis, halton_parameter_vector,
    joint_refinement_has_parallel_worker_capacity, joint_refinement_parallel_worker_count,
    ordered_parameter_bits, planned_candidate_work_item_labels, with_joint_refinement_thread_pool,
};
pub(crate) use state::{
    JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND, JOINT_REFINEMENT_PARAMETER_FAMILY_COUNT,
    JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR, JOINT_REFINEMENT_PRESSURE_RISK_PRICE_LOG_DENSITY_BOUND,
    JointRefinementActiveParameterLayout, JointRefinementAggregatedLikelihoodContextCacheKey,
    JointRefinementAggregatedLikelihoodContexts, JointRefinementCandidateEvaluation,
    JointRefinementCandidateOutcome, JointRefinementCoarseConsistencySummary,
    JointRefinementContext, JointRefinementDeterministicCacheKey,
    JointRefinementDeterministicEvaluation, JointRefinementFidelityTier,
    JointRefinementObjectiveCacheKey, JointRefinementObservedData,
    JointRefinementObservedReturnShapeStatisticCacheKey, JointRefinementOptionPricingCacheKey,
    JointRefinementOptionTierPricingArtifact, JointRefinementProfilingState,
    JointRefinementRawAdmissibilityMetrics, JointRefinementReturnShapeHorizon,
    JointRefinementReturnShapeStatisticEstimate, JointRefinementSeedSummaryCacheKey,
    JointRefinementStageRuntimeHandoff,
};
#[cfg(test)]
pub(crate) use test_support::{test_joint_candidate_outcome, test_joint_fitted_parameters};
