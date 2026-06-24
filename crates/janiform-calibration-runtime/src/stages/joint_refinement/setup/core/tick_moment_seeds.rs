use janiform_foundations::numeric::clamp_point_in_place as clamp_in_place;

use crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals;
use crate::stages::return_components::{
    NigShapeSearchPoint, ReturnComponentsStageContinuousReturnCalibrationSample,
    build_nig_moment_seed_configurations, nig_log_alpha_bounds_for_calibration_sample,
    nig_search_point_from_configuration,
    rescale_continuous_return_calibration_sample_business_time_scales,
};

use super::super::super::execution::{
    JointRefinementActiveParameterLayout, JointRefinementContext, JointRefinementObservedData,
    rescaled_observed_tick_calibration_sample_for_modeled_rate,
};
use super::activity::apply_activity_alignment;
use super::decode::parameter_vector_to_fitted_parameters;
use super::nig_candidate_seeds::apply_nig_search_point;
use janiform_calibration_artifacts::CalibrationOptimizerSettings;
pub(super) fn joint_tick_moment_candidate_seeds(
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    activity_log_shift: f64,
    joint_context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<(String, Vec<f64>)> {
    if !joint_context
        .baseline_modeled_open_event_rate_per_hour
        .is_finite()
        || joint_context.baseline_modeled_open_event_rate_per_hour <= 0.0
    {
        return Vec::new();
    }

    let baseline_tick_moment_calibration_sample = baseline_15m_calibration_sample_for_context(
        joint_context,
        joint_context.baseline_modeled_open_event_rate_per_hour,
    );

    let mut candidate_seeds = joint_tick_shape_alpha_boundary_candidate_seeds(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        joint_context,
        joint_context.baseline_modeled_open_event_rate_per_hour,
        None,
    );

    candidate_seeds.extend(
        projected_tick_moment_nig_search_points(ProjectedTickMomentNigSearchPointsInput {
            calibration_sample: &baseline_tick_moment_calibration_sample,
            observed_data: &joint_context.observed_data,
            baseline_modeled_open_event_rate_per_hour: joint_context
                .baseline_modeled_open_event_rate_per_hour,
            target_modeled_open_event_rate_per_hour: joint_context
                .baseline_modeled_open_event_rate_per_hour,
            solver_settings,
        })
        .into_iter()
        .enumerate()
        .map(|(configuration_index, projected_search_point)| {
            let mut baseline_candidate_seed = baseline_parameter_vector.to_vec();

            apply_nig_search_point(
                baseline_candidate_seed.as_mut_slice(),
                active_parameter_layout,
                projected_search_point,
            );

            clamp_in_place(
                baseline_candidate_seed.as_mut_slice(),
                lower_bounds,
                upper_bounds,
            );

            (
                format!("tick_moment_{:02}", configuration_index + 1),
                baseline_candidate_seed,
            )
        })
        .collect::<Vec<_>>(),
    );

    let mut activity_aligned_parameter_vector = baseline_parameter_vector.to_vec();

    apply_activity_alignment(
        activity_aligned_parameter_vector.as_mut_slice(),
        active_parameter_layout,
        activity_log_shift,
    );

    clamp_in_place(
        activity_aligned_parameter_vector.as_mut_slice(),
        lower_bounds,
        upper_bounds,
    );

    let Some(activity_aligned_fitted_parameters) = parameter_vector_to_fitted_parameters(
        activity_aligned_parameter_vector.as_slice(),
        joint_context,
    ) else {
        return candidate_seeds;
    };

    let Some(modeled_open_event_rate_per_hour) =
        crate::stages::hawkes::stationary_open_event_rate_per_hour(
            &activity_aligned_fitted_parameters.hawkes_fitted_parameters,
        )
    else {
        return candidate_seeds;
    };

    let activity_aligned_15m_calibration_sample = baseline_15m_calibration_sample_for_context(
        joint_context,
        modeled_open_event_rate_per_hour,
    );

    candidate_seeds.extend(joint_tick_shape_alpha_boundary_candidate_seeds(
        activity_aligned_parameter_vector.as_slice(),
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        joint_context,
        modeled_open_event_rate_per_hour,
        Some("activity_aligned"),
    ));

    candidate_seeds.extend(
        projected_tick_moment_nig_search_points(ProjectedTickMomentNigSearchPointsInput {
            calibration_sample: &activity_aligned_15m_calibration_sample,
            observed_data: &joint_context.observed_data,
            baseline_modeled_open_event_rate_per_hour: joint_context
                .baseline_modeled_open_event_rate_per_hour,
            target_modeled_open_event_rate_per_hour: modeled_open_event_rate_per_hour,
            solver_settings,
        })
        .into_iter()
        .enumerate()
        .map(|(configuration_index, projected_search_point)| {
            let mut activity_aligned_candidate_seed = activity_aligned_parameter_vector.clone();

            apply_nig_search_point(
                activity_aligned_candidate_seed.as_mut_slice(),
                active_parameter_layout,
                projected_search_point,
            );

            clamp_in_place(
                activity_aligned_candidate_seed.as_mut_slice(),
                lower_bounds,
                upper_bounds,
            );

            (
                format!(
                    "tick_moment_{:02}_activity_aligned",
                    configuration_index + 1
                ),
                activity_aligned_candidate_seed,
            )
        }),
    );

    candidate_seeds
}

fn joint_tick_shape_alpha_boundary_candidate_seeds(
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    joint_context: &JointRefinementContext,
    target_modeled_open_event_rate_per_hour: f64,
    seed_suffix: Option<&str>,
) -> Vec<(String, Vec<f64>)> {
    if !joint_context
        .baseline_modeled_open_event_rate_per_hour
        .is_finite()
        || joint_context.baseline_modeled_open_event_rate_per_hour <= 0.0
        || !target_modeled_open_event_rate_per_hour.is_finite()
        || target_modeled_open_event_rate_per_hour <= 0.0
    {
        return Vec::new();
    }

    let tick_calibration_sample = rescaled_observed_tick_calibration_sample_for_modeled_rate(
        &joint_context.observed_data,
        joint_context.baseline_modeled_open_event_rate_per_hour,
        target_modeled_open_event_rate_per_hour,
    );

    let (tick_log_alpha_lower_bound, tick_log_alpha_upper_bound) =
        nig_log_alpha_bounds_for_calibration_sample(&tick_calibration_sample);

    [
        ("tick_shape_alpha_lower", tick_log_alpha_lower_bound),
        ("tick_shape_alpha_upper", tick_log_alpha_upper_bound),
    ]
    .into_iter()
    .map(|(seed_label, alpha_coordinate)| {
        let mut candidate_seed = baseline_parameter_vector.to_vec();
        candidate_seed[active_parameter_layout.nig_block[0]] = alpha_coordinate;
        clamp_in_place(candidate_seed.as_mut_slice(), lower_bounds, upper_bounds);

        (
            tick_shape_alpha_boundary_seed_id(seed_label, seed_suffix),
            candidate_seed,
        )
    })
    .collect()
}

fn tick_shape_alpha_boundary_seed_id(seed_label: &str, seed_suffix: Option<&str>) -> String {
    match seed_suffix {
        Some(seed_suffix) => format!("{seed_label}_{seed_suffix}"),
        None => seed_label.to_string(),
    }
}

fn baseline_15m_calibration_sample_for_context(
    joint_context: &JointRefinementContext,
    modeled_open_event_rate_per_hour: f64,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
    build_continuous_calibration_sample_from_aggregated_intervals(
        joint_context
            .observed_data
            .observed_15m_intervals
            .as_slice(),
        modeled_open_event_rate_per_hour,
        joint_context
            .observed_data
            .observed_open_event_rate_per_hour,
        joint_context.observed_data.fallback_sell_share_proxy,
    )
}

pub(super) fn activity_aligned_calibration_sample(
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    activity_log_shift: f64,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
    rescale_continuous_return_calibration_sample_business_time_scales(
        baseline_15m_calibration_sample,
        activity_log_shift.exp(),
    )
}

pub(super) struct ProjectedTickMomentNigSearchPointsInput<'a> {
    pub(super) calibration_sample: &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(super) observed_data: &'a JointRefinementObservedData,
    pub(super) baseline_modeled_open_event_rate_per_hour: f64,
    pub(super) target_modeled_open_event_rate_per_hour: f64,
    pub(super) solver_settings: &'a CalibrationOptimizerSettings,
}

pub(super) fn projected_tick_moment_nig_search_points(
    input: ProjectedTickMomentNigSearchPointsInput<'_>,
) -> Vec<NigShapeSearchPoint> {
    let ProjectedTickMomentNigSearchPointsInput {
        calibration_sample,
        observed_data,
        baseline_modeled_open_event_rate_per_hour,
        target_modeled_open_event_rate_per_hour,
        solver_settings,
    } = input;

    if !baseline_modeled_open_event_rate_per_hour.is_finite()
        || baseline_modeled_open_event_rate_per_hour <= 0.0
        || !target_modeled_open_event_rate_per_hour.is_finite()
        || target_modeled_open_event_rate_per_hour <= 0.0
    {
        return Vec::new();
    }

    let tick_sample_business_time_scaled =
        rescaled_observed_tick_calibration_sample_for_modeled_rate(
            observed_data,
            baseline_modeled_open_event_rate_per_hour,
            target_modeled_open_event_rate_per_hour,
        );

    build_nig_moment_seed_configurations(&tick_sample_business_time_scaled, solver_settings)
        .into_iter()
        .filter_map(|configuration| {
            nig_search_point_from_configuration(calibration_sample, configuration)
        })
        .collect()
}
