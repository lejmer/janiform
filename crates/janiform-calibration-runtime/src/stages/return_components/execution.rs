mod candidate_generation;
mod close_open_gap;
mod diagnostics;
mod evaluation;
mod signals;
mod stage;
#[cfg(test)]
mod tests;

pub(crate) use diagnostics::{
    ReturnComponentsStageReturnDistributionSummary, arithmetic_mean_signed,
    round_negative_candidate_value, round_positive_candidate_value, round_signed_candidate_value,
    summarize_return_distribution, theoretical_net_process_moments,
};
pub use evaluation::evaluate_return_components_exact_candidate;
pub(crate) use evaluation::{
    ReturnComponentsStageJointRefinementObservedSignalPreparation,
    prepare_observed_signal_inputs_for_joint_refinement,
};
pub(crate) use signals::ReturnComponentsStageSortedSignalInputs;
pub(crate) use signals::resolve_activity_calibration_context;
pub use stage::run_return_components_calibration_stage;
pub(crate) use stage::run_return_components_calibration_stage_with_progress_observer;

#[cfg(test)]
pub(crate) use close_open_gap::derive_close_open_gap_candidates;
#[cfg(test)]
pub(crate) use diagnostics::{
    build_activity_coupling_diagnostic, build_gap_diagnostic, build_jump_diagnostic,
    build_price_range_sanity_diagnostic, build_surprise_arrival_diagnostic,
    default_surprise_deterministic_intensity_profile,
};
#[cfg(test)]
pub(crate) use signals::{
    sort_candidate_generation_signals, summarize_candidate_generation_signals,
};
