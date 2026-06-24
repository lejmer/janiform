use janiform_foundations::numeric::clamp_point_in_place as clamp_in_place;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use crate::stages::return_components::{
    NigShapeSearchPoint, ReturnComponentsStageCandidateDiagnostic,
    ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
    ReturnComponentsStageContinuousReturnCalibrationSample, ReturnComponentsStageFittedPayload,
    nig_search_point_from_configuration,
};

use super::super::super::execution::JointRefinementActiveParameterLayout;
use super::activity::apply_activity_alignment;
pub(super) struct JointNigCandidateSeedsBuildInput<'a> {
    pub(super) baseline_parameter_vector: &'a [f64],
    pub(super) active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    pub(super) lower_bounds: &'a [f64],
    pub(super) upper_bounds: &'a [f64],
    pub(super) baseline_15m_calibration_sample:
        &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(super) activity_aligned_15m_calibration_sample:
        &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(super) return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    pub(super) activity_log_shift: f64,
    pub(super) maximum_seed_count: usize,
}

pub(super) fn joint_nig_basin_candidate_seeds(
    input: JointNigCandidateSeedsBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let JointNigCandidateSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        activity_aligned_15m_calibration_sample,
        return_component_basin_diagnostics,
        activity_log_shift,
        maximum_seed_count,
    } = input;

    return_component_basin_diagnostics
        .iter()
        .take(maximum_seed_count.div_ceil(2))
        .filter_map(|diagnostic| {
            project_basin_diagnostic_into_joint_nig_coordinates(
                baseline_15m_calibration_sample,
                diagnostic,
            )
            .map(|baseline_search_point| {
                let activity_aligned_search_point =
                    project_basin_diagnostic_into_joint_nig_coordinates(
                        activity_aligned_15m_calibration_sample,
                        diagnostic,
                    );

                (
                    diagnostic,
                    baseline_search_point,
                    activity_aligned_search_point,
                )
            })
        })
        .flat_map(
            |(diagnostic, baseline_search_point, activity_aligned_search_point)| {
                let mut baseline_nig_seed = baseline_parameter_vector.to_vec();
                apply_nig_search_point(
                    baseline_nig_seed.as_mut_slice(),
                    active_parameter_layout,
                    baseline_search_point,
                );
                clamp_in_place(baseline_nig_seed.as_mut_slice(), lower_bounds, upper_bounds);

                let mut seeds = vec![(
                    format!("nig_basin_{}", diagnostic.family_label),
                    baseline_nig_seed,
                )];

                if let Some(activity_aligned_search_point) = activity_aligned_search_point {
                    let mut activity_aligned_nig_seed = baseline_parameter_vector.to_vec();
                    apply_nig_search_point(
                        activity_aligned_nig_seed.as_mut_slice(),
                        active_parameter_layout,
                        activity_aligned_search_point,
                    );
                    apply_activity_alignment(
                        activity_aligned_nig_seed.as_mut_slice(),
                        active_parameter_layout,
                        activity_log_shift,
                    );
                    clamp_in_place(
                        activity_aligned_nig_seed.as_mut_slice(),
                        lower_bounds,
                        upper_bounds,
                    );

                    seeds.push((
                        format!("nig_basin_{}_activity_aligned", diagnostic.family_label),
                        activity_aligned_nig_seed,
                    ));
                }

                seeds
            },
        )
        .collect()
}

pub(super) fn joint_nig_alpha_delta_candidate_seeds(
    input: JointNigCandidateSeedsBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let JointNigCandidateSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        activity_aligned_15m_calibration_sample,
        return_component_basin_diagnostics,
        activity_log_shift,
        maximum_seed_count,
    } = input;

    return_component_basin_diagnostics
        .iter()
        .take(maximum_seed_count.div_ceil(2))
        .filter_map(|diagnostic| {
            project_basin_diagnostic_into_joint_nig_coordinates(
                baseline_15m_calibration_sample,
                diagnostic,
            )
            .map(|baseline_search_point| {
                let activity_aligned_search_point =
                    project_basin_diagnostic_into_joint_nig_coordinates(
                        activity_aligned_15m_calibration_sample,
                        diagnostic,
                    );

                (
                    diagnostic,
                    baseline_search_point,
                    activity_aligned_search_point,
                )
            })
        })
        .flat_map(
            |(diagnostic, baseline_search_point, activity_aligned_search_point)| {
                let mut alpha_delta_seed = baseline_parameter_vector.to_vec();

                apply_nig_alpha_delta_search_point(
                    alpha_delta_seed.as_mut_slice(),
                    active_parameter_layout,
                    baseline_search_point,
                );

                clamp_in_place(alpha_delta_seed.as_mut_slice(), lower_bounds, upper_bounds);

                let mut seeds = vec![(
                    format!("nig_alpha_delta_{}", diagnostic.family_label),
                    alpha_delta_seed,
                )];

                if let Some(activity_aligned_search_point) = activity_aligned_search_point {
                    let mut activity_aligned_alpha_delta_seed = baseline_parameter_vector.to_vec();

                    apply_nig_alpha_delta_search_point(
                        activity_aligned_alpha_delta_seed.as_mut_slice(),
                        active_parameter_layout,
                        activity_aligned_search_point,
                    );

                    apply_activity_alignment(
                        activity_aligned_alpha_delta_seed.as_mut_slice(),
                        active_parameter_layout,
                        activity_log_shift,
                    );

                    clamp_in_place(
                        activity_aligned_alpha_delta_seed.as_mut_slice(),
                        lower_bounds,
                        upper_bounds,
                    );

                    seeds.push((
                        format!(
                            "nig_alpha_delta_{}_activity_aligned",
                            diagnostic.family_label
                        ),
                        activity_aligned_alpha_delta_seed,
                    ));
                }

                seeds
            },
        )
        .collect()
}

pub(in crate::stages::joint_refinement) fn selected_return_components_candidate(
    return_components_payload: &ReturnComponentsStageFittedPayload,
) -> Option<&ReturnComponentsStageCandidateDiagnostic> {
    return_components_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == return_components_payload.selected_candidate_id)
}

pub(in crate::stages::joint_refinement) fn project_basin_diagnostic_into_joint_nig_coordinates(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    diagnostic: &ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
) -> Option<NigShapeSearchPoint> {
    if !nig_basin_coordinates_are_finite(diagnostic) {
        return None;
    }

    let configuration = NigSharedPlusShiftConfiguration::try_new(
        diagnostic.final_alpha,
        diagnostic.final_beta_0,
        diagnostic.final_delta,
        0.0,
        diagnostic.final_delta_beta,
    )
    .ok()?;

    nig_search_point_from_configuration(calibration_sample, configuration)
}

fn nig_basin_coordinates_are_finite(
    diagnostic: &ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
) -> bool {
    [
        diagnostic.final_log_alpha,
        diagnostic.final_buy_skew_coordinate,
        diagnostic.final_sell_relative_coordinate,
        diagnostic.final_log_delta_offset,
    ]
    .into_iter()
    .all(f64::is_finite)
}

pub(super) fn apply_nig_search_point(
    parameter_vector: &mut [f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    search_point: NigShapeSearchPoint,
) {
    parameter_vector[active_parameter_layout.nig_block[0]] = search_point.log_alpha;
    parameter_vector[active_parameter_layout.nig_block[1]] = search_point.buy_skew_coordinate;
    parameter_vector[active_parameter_layout.nig_block[2]] = search_point.sell_relative_coordinate;
    parameter_vector[active_parameter_layout.nig_block[3]] = search_point.log_delta_offset;
}

fn apply_nig_alpha_delta_search_point(
    parameter_vector: &mut [f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    search_point: NigShapeSearchPoint,
) {
    parameter_vector[active_parameter_layout.nig_block[0]] = search_point.log_alpha;
    parameter_vector[active_parameter_layout.nig_block[3]] = search_point.log_delta_offset;
}

pub(in crate::stages::joint_refinement) fn apply_nig_log_delta_offset_shift(
    parameter_vector: &mut [f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    log_delta_offset_shift: f64,
) {
    parameter_vector[active_parameter_layout.nig_block[3]] += log_delta_offset_shift;
}
