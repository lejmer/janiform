mod core;
mod long_variance_search;
mod return_clock_candidate_seeds;
#[cfg(test)]
mod test_support;

pub(super) use core::{
    CandidateSeedSummariesCacheInput, CandidateSeedsBuildInput, ParameterBoundsBuildInput,
    apply_option_pricing_kernel_bounds, build_active_parameter_layout,
    build_baseline_parameter_vector, build_candidate_seeds, build_parameter_bounds,
    cached_seed_summaries_for_candidate_seeds, checked_parameter_vector_to_fitted_parameters,
    joint_refinement_final_simulation_seed_count, observed_activity_log_shift,
    observed_activity_log_shift_for_joint_context, parameter_vector_to_fitted_parameters,
    selected_hawkes_observed_open_event_rate_per_hour, selected_return_components_candidate,
    stage_input_to_activity_input,
};

pub(crate) use core::build_surprise_multipliers_for_interval_grid;

#[cfg(test)]
pub(super) use core::{
    CandidateSeedsWithHaltonAcceptanceBuildInput, build_candidate_seeds_with_halton_acceptance,
    candidate_screening_max_parallelism_for_test, parameter_vector_to_nig_search_point,
    project_basin_diagnostic_into_joint_nig_coordinates,
};
