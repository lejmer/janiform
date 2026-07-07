use std::sync::Mutex;
use std::sync::mpsc::Sender;

use janiform_foundations::numeric::clamp_point_in_place as clamp_in_place;

use crate::stages::return_components::{
    ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
    ReturnComponentsStageContinuousReturnCalibrationSample,
};

use super::super::super::contracts::{
    JointRefinementStageObjectiveTermDiagnostic, JointRefinementStageProgressUpdate,
};
use super::super::super::execution::{
    JointRefinementActiveParameterLayout, JointRefinementContext, deduplicate_candidate_seeds,
};
use super::super::super::objective::JointRefinementCandidateAdmissionScreening;
use super::super::long_variance_search::joint_long_variance_candidate_seeds;
use super::super::return_clock_candidate_seeds::{
    JointReturnClockExposureSeedsBuildInput, ReturnClockExposureCandidateSeedProfile,
    joint_return_clock_exposure_candidate_seeds, return_clock_exposure_candidate_seed_profile,
};
use super::activity::apply_activity_alignment;
#[cfg(test)]
use super::candidate_screening::candidate_admission_screening_from_scalar;
use super::candidate_screening::{ScreenedCandidate, select_screened_candidate_seeds};
use super::candidate_screening_jobs::{
    HaltonCandidateScreeningInput, screen_candidate_seeds, screen_halton_candidate_seeds,
    screened_exploratory_candidate_seed_score,
};
use super::nig_candidate_seeds::{
    JointNigCandidateSeedsBuildInput, joint_nig_alpha_delta_candidate_seeds,
    joint_nig_basin_candidate_seeds,
};
use super::progress::{emit_candidate_screening_progress, emit_candidate_seed_generation_progress};
use super::return_scale_alignment::{
    JointReturnScaleAlignmentSeedsBuildInput, joint_return_scale_alignment_candidate_seeds,
};
use super::tick_moment_seeds::{
    activity_aligned_calibration_sample, joint_tick_moment_candidate_seeds,
};
use janiform_calibration_artifacts::CalibrationOptimizerSettings;

const PROTECTED_CANDIDATE_SEED_IDS: [&str; 2] = ["incumbent_local_search", "activity_aligned"];
pub(in crate::stages::joint_refinement) fn build_candidate_seeds(
    input: CandidateSeedsBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let CandidateSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
        joint_context,
        solver_settings,
        candidate_budget,
        progress_sender,
    } = input;

    emit_candidate_seed_generation_progress(
        progress_sender,
        0,
        2,
        Some("core_candidate_seed_generation"),
    );

    let return_clock_seed_profile = return_clock_exposure_candidate_seed_profile(joint_context);

    let mut candidate_seeds = build_core_candidate_seeds(CoreCandidateSeedsInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
        return_clock_seed_profile,
    });

    emit_candidate_seed_generation_progress(
        progress_sender,
        1,
        2,
        Some("tick_moment_seed_generation"),
    );

    candidate_seeds.extend(joint_tick_moment_candidate_seeds(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        activity_log_shift,
        joint_context,
        solver_settings,
    ));

    emit_candidate_seed_generation_progress(progress_sender, 2, 2, Some("candidate_seeds_built"));

    let candidate_seed_score = |_: &str, candidate_seed: &[f64]| {
        screened_exploratory_candidate_seed_score(candidate_seed, joint_context)
    };

    finalize_candidate_seeds(CandidateSeedFinalizationInput {
        candidate_seeds,
        baseline_parameter_vector,
        lower_bounds,
        upper_bounds,
        candidate_budget,
        candidate_seed_score: &candidate_seed_score,
        protected_candidate_ids: &PROTECTED_CANDIDATE_SEED_IDS,
        joint_context: Some(joint_context),
        progress_sender,
    })
}

pub(in crate::stages::joint_refinement) struct CandidateSeedsBuildInput<'a> {
    pub(in crate::stages::joint_refinement) baseline_parameter_vector: &'a [f64],
    pub(in crate::stages::joint_refinement) active_parameter_layout:
        &'a JointRefinementActiveParameterLayout,
    pub(in crate::stages::joint_refinement) lower_bounds: &'a [f64],
    pub(in crate::stages::joint_refinement) upper_bounds: &'a [f64],
    pub(in crate::stages::joint_refinement) baseline_15m_calibration_sample:
        &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(in crate::stages::joint_refinement) return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    pub(in crate::stages::joint_refinement) baseline_final_objective_terms:
        &'a [JointRefinementStageObjectiveTermDiagnostic],
    pub(in crate::stages::joint_refinement) activity_log_shift: f64,
    pub(in crate::stages::joint_refinement) joint_context: &'a JointRefinementContext,
    pub(in crate::stages::joint_refinement) solver_settings: &'a CalibrationOptimizerSettings,
    pub(in crate::stages::joint_refinement) candidate_budget: usize,
    pub(in crate::stages::joint_refinement) progress_sender:
        Option<&'a Sender<JointRefinementStageProgressUpdate>>,
}

#[cfg(test)]
pub(in crate::stages::joint_refinement) fn build_candidate_seeds_with_halton_acceptance(
    input: CandidateSeedsWithHaltonAcceptanceBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let CandidateSeedsWithHaltonAcceptanceBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
        candidate_budget,
        halton_seed_is_accepted,
    } = input;

    let halton_seed_is_accepted = Mutex::new(halton_seed_is_accepted);

    let candidate_seeds = build_core_candidate_seeds(CoreCandidateSeedsInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
        return_clock_seed_profile: None,
    });

    let candidate_seed_score = |candidate_id: &str, candidate_seed: &[f64]| {
        if !candidate_id.starts_with("halton_") {
            return candidate_admission_screening_from_scalar(0.0);
        }

        let Ok(mut halton_seed_is_accepted) = halton_seed_is_accepted.lock() else {
            return None;
        };

        halton_seed_is_accepted(candidate_seed).then(|| {
            candidate_admission_screening_from_scalar(0.0)
                .expect("finite scalar screening score should build")
        })
    };

    finalize_candidate_seeds(CandidateSeedFinalizationInput {
        candidate_seeds,
        baseline_parameter_vector,
        lower_bounds,
        upper_bounds,
        candidate_budget,
        candidate_seed_score: &candidate_seed_score,
        protected_candidate_ids: &PROTECTED_CANDIDATE_SEED_IDS,
        joint_context: None,
        progress_sender: None,
    })
}

#[cfg(test)]
type HaltonSeedAcceptancePredicate<'a> = Box<dyn FnMut(&[f64]) -> bool + Send + 'a>;

#[cfg(test)]
pub(in crate::stages::joint_refinement) struct CandidateSeedsWithHaltonAcceptanceBuildInput<'a> {
    pub(in crate::stages::joint_refinement) baseline_parameter_vector: &'a [f64],
    pub(in crate::stages::joint_refinement) active_parameter_layout:
        &'a JointRefinementActiveParameterLayout,
    pub(in crate::stages::joint_refinement) lower_bounds: &'a [f64],
    pub(in crate::stages::joint_refinement) upper_bounds: &'a [f64],
    pub(in crate::stages::joint_refinement) baseline_15m_calibration_sample:
        &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(in crate::stages::joint_refinement) return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    pub(in crate::stages::joint_refinement) baseline_final_objective_terms:
        &'a [JointRefinementStageObjectiveTermDiagnostic],
    pub(in crate::stages::joint_refinement) activity_log_shift: f64,
    pub(in crate::stages::joint_refinement) candidate_budget: usize,
    pub(in crate::stages::joint_refinement) halton_seed_is_accepted:
        HaltonSeedAcceptancePredicate<'a>,
}

struct CoreCandidateSeedsInput<'a> {
    baseline_parameter_vector: &'a [f64],
    active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    baseline_15m_calibration_sample: &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    baseline_final_objective_terms: &'a [JointRefinementStageObjectiveTermDiagnostic],
    activity_log_shift: f64,
    return_clock_seed_profile: Option<ReturnClockExposureCandidateSeedProfile>,
}

fn build_core_candidate_seeds(input: CoreCandidateSeedsInput<'_>) -> Vec<(String, Vec<f64>)> {
    let CoreCandidateSeedsInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
        return_clock_seed_profile,
    } = input;

    let mut incumbent_local_search = baseline_parameter_vector.to_vec();

    clamp_in_place(
        incumbent_local_search.as_mut_slice(),
        lower_bounds,
        upper_bounds,
    );

    let mut activity_aligned = baseline_parameter_vector.to_vec();

    apply_activity_alignment(
        activity_aligned.as_mut_slice(),
        active_parameter_layout,
        activity_log_shift,
    );

    clamp_in_place(activity_aligned.as_mut_slice(), lower_bounds, upper_bounds);

    let mut midpoint = lower_bounds
        .iter()
        .zip(upper_bounds.iter())
        .map(|(lower_bound, upper_bound)| 0.5 * (lower_bound + upper_bound))
        .collect::<Vec<_>>();

    clamp_in_place(midpoint.as_mut_slice(), lower_bounds, upper_bounds);

    let mut candidate_seeds = vec![("incumbent_local_search".to_string(), incumbent_local_search)];

    candidate_seeds.push(("activity_aligned".to_string(), activity_aligned));

    candidate_seeds.extend(joint_return_scale_alignment_candidate_seeds(
        JointReturnScaleAlignmentSeedsBuildInput {
            baseline_parameter_vector,
            active_parameter_layout,
            lower_bounds,
            upper_bounds,
            baseline_final_objective_terms,
            activity_log_shift,
        },
    ));

    if let Some(return_clock_seed_profile) = return_clock_seed_profile {
        candidate_seeds.extend(joint_return_clock_exposure_candidate_seeds(
            JointReturnClockExposureSeedsBuildInput {
                baseline_parameter_vector,
                active_parameter_layout,
                lower_bounds,
                upper_bounds,
                return_clock_seed_profile,
            },
        ));
    }

    candidate_seeds.push(("midpoint".to_string(), midpoint));

    let activity_aligned_15m_calibration_sample =
        activity_aligned_calibration_sample(baseline_15m_calibration_sample, activity_log_shift);

    candidate_seeds.extend(joint_nig_basin_candidate_seeds(
        JointNigCandidateSeedsBuildInput {
            baseline_parameter_vector,
            active_parameter_layout,
            lower_bounds,
            upper_bounds,
            baseline_15m_calibration_sample,
            activity_aligned_15m_calibration_sample: &activity_aligned_15m_calibration_sample,
            return_component_basin_diagnostics,
            activity_log_shift,
            maximum_seed_count: 4,
        },
    ));

    candidate_seeds.extend(joint_nig_alpha_delta_candidate_seeds(
        JointNigCandidateSeedsBuildInput {
            baseline_parameter_vector,
            active_parameter_layout,
            lower_bounds,
            upper_bounds,
            baseline_15m_calibration_sample,
            activity_aligned_15m_calibration_sample: &activity_aligned_15m_calibration_sample,
            return_component_basin_diagnostics,
            activity_log_shift,
            maximum_seed_count: 2,
        },
    ));

    candidate_seeds.extend(joint_long_variance_candidate_seeds(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
    ));

    candidate_seeds
}
fn finalize_candidate_seeds(input: CandidateSeedFinalizationInput<'_>) -> Vec<(String, Vec<f64>)> {
    let CandidateSeedFinalizationInput {
        candidate_seeds,
        baseline_parameter_vector,
        lower_bounds,
        upper_bounds,
        candidate_budget,
        candidate_seed_score,
        protected_candidate_ids,
        joint_context,
        progress_sender,
    } = input;

    let target_candidate_count = candidate_budget.max(1);
    let deduplicated_candidates = deduplicate_candidate_seeds(candidate_seeds);

    let total_screening_units = deduplicated_candidates
        .len()
        .saturating_add(target_candidate_count);

    emit_candidate_screening_progress(
        progress_sender,
        0,
        total_screening_units,
        Some("candidate_frontier"),
    );

    let completed_screening_units = Mutex::new(0usize);

    let screened_candidate_seeds = screen_candidate_seeds(
        deduplicated_candidates.as_slice(),
        &candidate_seed_score,
        joint_context,
        progress_sender,
        &completed_screening_units,
        total_screening_units,
    );

    let mut screened_halton_candidates = Vec::new();

    if screened_candidate_seeds.len() < target_candidate_count {
        screened_halton_candidates = screen_halton_candidate_seeds(HaltonCandidateScreeningInput {
            baseline_parameter_vector,
            lower_bounds,
            upper_bounds,
            target_candidate_count,
            source_order_offset: deduplicated_candidates.len(),
            halton_seed_score: &candidate_seed_score,
            joint_context,
            progress_sender,
            completed_screening_units: &completed_screening_units,
            total_screening_units,
        });
    }

    emit_candidate_screening_progress(
        progress_sender,
        total_screening_units,
        total_screening_units,
        Some("candidate_seeds_screened"),
    );

    let selected_candidate_seeds = if screened_candidate_seeds.len() < target_candidate_count {
        let remaining_candidate_count = target_candidate_count - screened_candidate_seeds.len();

        let mut selected_candidate_seeds =
            screened_candidate_seeds_to_seed_pairs(screened_candidate_seeds);

        selected_candidate_seeds.extend(select_screened_candidate_seeds(
            screened_halton_candidates,
            remaining_candidate_count,
            lower_bounds,
            upper_bounds,
            &[],
        ));

        selected_candidate_seeds
    } else {
        select_screened_candidate_seeds(
            screened_candidate_seeds,
            target_candidate_count,
            lower_bounds,
            upper_bounds,
            protected_candidate_ids,
        )
    };

    deduplicate_candidate_seeds(selected_candidate_seeds)
        .into_iter()
        .take(target_candidate_count)
        .collect()
}

fn screened_candidate_seeds_to_seed_pairs(
    screened_candidates: Vec<ScreenedCandidate>,
) -> Vec<(String, Vec<f64>)> {
    screened_candidates
        .into_iter()
        .map(|candidate| (candidate.candidate_id, candidate.parameter_vector))
        .collect()
}

struct CandidateSeedFinalizationInput<'a> {
    candidate_seeds: Vec<(String, Vec<f64>)>,
    baseline_parameter_vector: &'a [f64],
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    candidate_budget: usize,
    candidate_seed_score:
        &'a (dyn Fn(&str, &[f64]) -> Option<JointRefinementCandidateAdmissionScreening> + Sync),
    protected_candidate_ids: &'a [&'a str],
    joint_context: Option<&'a JointRefinementContext>,
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
}
