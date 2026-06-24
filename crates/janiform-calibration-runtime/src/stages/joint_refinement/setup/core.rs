mod activity;
mod bounds;
mod candidate_screening;
mod candidate_screening_jobs;
mod candidate_seeds;
mod decode;
mod layout;
mod nig_candidate_seeds;
mod progress;
mod return_scale_alignment;
mod seed_summaries;
mod tick_moment_seeds;

pub(in crate::stages::joint_refinement) use activity::{
    observed_activity_log_shift, observed_activity_log_shift_for_joint_context,
    selected_hawkes_observed_open_event_rate_per_hour, stage_input_to_activity_input,
};
pub(in crate::stages::joint_refinement) use bounds::{
    ParameterBoundsBuildInput, apply_option_pricing_kernel_bounds, build_parameter_bounds,
};
#[cfg(test)]
pub(in crate::stages::joint_refinement) use candidate_screening_jobs::candidate_screening_max_parallelism_for_test;
pub(in crate::stages::joint_refinement) use candidate_seeds::{
    CandidateSeedsBuildInput, build_candidate_seeds,
};
#[cfg(test)]
pub(in crate::stages::joint_refinement) use candidate_seeds::{
    CandidateSeedsWithHaltonAcceptanceBuildInput, build_candidate_seeds_with_halton_acceptance,
};
pub(in crate::stages::joint_refinement) use decode::{
    checked_parameter_vector_to_fitted_parameters, parameter_vector_to_fitted_parameters,
};
#[cfg(test)]
pub(in crate::stages::joint_refinement) use layout::parameter_vector_to_nig_search_point;
pub(in crate::stages::joint_refinement) use layout::{
    build_active_parameter_layout, build_baseline_parameter_vector,
};
#[cfg(test)]
pub(in crate::stages::joint_refinement) use nig_candidate_seeds::project_basin_diagnostic_into_joint_nig_coordinates;
pub(in crate::stages::joint_refinement) use nig_candidate_seeds::{
    apply_nig_log_delta_offset_shift, selected_return_components_candidate,
};
pub(crate) use seed_summaries::build_surprise_multipliers_for_interval_grid;
pub(in crate::stages::joint_refinement) use seed_summaries::{
    CandidateSeedSummariesCacheInput, cached_seed_summaries_for_candidate_seeds,
    joint_refinement_final_simulation_seed_count,
};
