use janiform_foundations::time::duration_nanos_to_hours;

use crate::stages::hawkes::HawkesStageFittedPayload;

use super::super::super::execution::{
    JointRefinementActiveParameterLayout, JointRefinementContext,
};
pub(super) fn apply_activity_alignment(
    parameter_vector: &mut [f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    activity_log_shift: f64,
) {
    if let Some(index) = active_parameter_layout.known_activity_scale_index {
        parameter_vector[index] = activity_log_shift;
    }

    if let Some(index) = active_parameter_layout.surprise_activity_scale_index {
        parameter_vector[index] = activity_log_shift;
    }

    if let Some(index) = active_parameter_layout.surprise_baseline_scale_index {
        parameter_vector[index] = activity_log_shift;
    }
}
pub(in crate::stages::joint_refinement) fn observed_activity_log_shift(
    observed_open_event_rate_per_hour: f64,
    modeled_open_event_rate_per_hour: f64,
) -> f64 {
    if !observed_open_event_rate_per_hour.is_finite()
        || observed_open_event_rate_per_hour <= 0.0
        || !modeled_open_event_rate_per_hour.is_finite()
        || modeled_open_event_rate_per_hour <= 0.0
    {
        return 0.0;
    }

    (observed_open_event_rate_per_hour / modeled_open_event_rate_per_hour).ln()
}

pub(in crate::stages::joint_refinement) fn observed_activity_log_shift_for_joint_context(
    joint_context: &JointRefinementContext,
) -> Option<f64> {
    let observed_total_count = joint_context
        .observed_data
        .observed_hourly_tick_counts
        .iter()
        .sum::<f64>();
    if !observed_total_count.is_finite() || observed_total_count <= 0.0 {
        return None;
    }

    let baseline_exposure = activity_exposure_for_simulation_template(joint_context)?;

    let baseline_expected_total_count =
        joint_context.baseline_modeled_open_event_rate_per_hour * baseline_exposure;
    if !baseline_expected_total_count.is_finite() || baseline_expected_total_count <= 0.0 {
        return None;
    }

    Some((observed_total_count / baseline_expected_total_count).ln())
}

fn activity_exposure_for_simulation_template(
    joint_context: &JointRefinementContext,
) -> Option<f64> {
    if joint_context.runtime_interval_seasonality_factors.len()
        != joint_context
            .observed_data
            .simulated_hourly_interval_template
            .len()
    {
        return None;
    }

    let mut exposure = 0.0;

    for (interval, interval_factor_grid) in joint_context
        .observed_data
        .simulated_hourly_interval_template
        .iter()
        .zip(joint_context.runtime_interval_seasonality_factors.iter())
    {
        if !interval_factor_grid.is_finite()
            || *interval_factor_grid < 0.0
            || interval.interval_end_utc_nanos < interval.interval_start_utc_nanos
        {
            return None;
        }

        let duration_hours = duration_nanos_to_hours(
            interval.interval_end_utc_nanos - interval.interval_start_utc_nanos,
        );

        exposure += *interval_factor_grid * duration_hours;
    }

    (exposure.is_finite() && exposure > 0.0).then_some(exposure)
}
pub(in crate::stages::joint_refinement) fn selected_hawkes_observed_open_event_rate_per_hour(
    hawkes_payload: &HawkesStageFittedPayload,
) -> Option<f64> {
    hawkes_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == hawkes_payload.selected_candidate_id)
        .map(|candidate| candidate.implied_metrics.observed_open_event_rate_per_hour)
        .or_else(|| {
            crate::stages::hawkes::stationary_open_event_rate_per_hour(
                &hawkes_payload.fitted_parameters,
            )
        })
}

pub(in crate::stages::joint_refinement) fn stage_input_to_activity_input(
    hawkes_input: &janiform_calibration_artifacts::CalibrationStagePayloadSource<
        HawkesStageFittedPayload,
    >,
) -> janiform_calibration_artifacts::CalibrationStagePayloadSource<HawkesStageFittedPayload> {
    // Intentional copy because joint refinement builds a nested return-components input that owns
    // the same upstream Hawkes source contract.
    hawkes_input.clone()
}
