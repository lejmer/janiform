use janiform_model_kernels::hawkes::HawkesKernelParameters;
use janiform_model_kernels::long_variance::LongVarianceParameters;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals;
use crate::stages::return_components::{
    NigShapeSearchPoint, ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
    ReturnComponentsStageContinuousReturnCalibrationSample,
    nig_log_alpha_bounds_for_calibration_sample,
    nig_search_bounds_for_calibration_sample_with_solver_settings,
    nig_search_point_from_configuration,
};

use super::super::super::execution::{
    JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND,
    JOINT_REFINEMENT_PRESSURE_RISK_PRICE_LOG_DENSITY_BOUND, JointRefinementActiveParameterLayout,
    JointRefinementObservedData, rescaled_observed_tick_calibration_sample_for_modeled_rate,
};
use super::super::long_variance_search::apply_long_variance_parameter_bounds;
use super::nig_candidate_seeds::project_basin_diagnostic_into_joint_nig_coordinates;
use super::tick_moment_seeds::{
    ProjectedTickMomentNigSearchPointsInput, projected_tick_moment_nig_search_points,
};
use janiform_calibration_artifacts::CalibrationOptimizerSettings;
pub(in crate::stages::joint_refinement) struct ParameterBoundsBuildInput<'a> {
    pub(in crate::stages::joint_refinement) active_parameter_layout:
        &'a JointRefinementActiveParameterLayout,
    pub(in crate::stages::joint_refinement) baseline_parameter_vector: &'a [f64],
    pub(in crate::stages::joint_refinement) baseline_15m_calibration_sample:
        &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(in crate::stages::joint_refinement) selected_return_nig_configuration:
        NigSharedPlusShiftConfiguration,
    pub(in crate::stages::joint_refinement) return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    pub(in crate::stages::joint_refinement) baseline_hawkes_kernel_parameters:
        HawkesKernelParameters,
    pub(in crate::stages::joint_refinement) baseline_long_variance_parameters:
        LongVarianceParameters,
    pub(in crate::stages::joint_refinement) observed_data: &'a JointRefinementObservedData,
    pub(in crate::stages::joint_refinement) baseline_modeled_open_event_rate_per_hour: f64,
    pub(in crate::stages::joint_refinement) solver_settings: &'a CalibrationOptimizerSettings,
}

pub(in crate::stages::joint_refinement) fn build_parameter_bounds(
    input: ParameterBoundsBuildInput<'_>,
) -> Result<(Vec<f64>, Vec<f64>), String> {
    let ParameterBoundsBuildInput {
        active_parameter_layout,
        baseline_parameter_vector,
        baseline_15m_calibration_sample,
        selected_return_nig_configuration,
        return_component_basin_diagnostics,
        baseline_hawkes_kernel_parameters,
        baseline_long_variance_parameters,
        observed_data,
        baseline_modeled_open_event_rate_per_hour,
        solver_settings,
    } = input;

    let mut lower_bounds = vec![0.0; baseline_parameter_vector.len()];
    let mut upper_bounds = vec![0.0; baseline_parameter_vector.len()];

    for index in &active_parameter_layout.hawkes_block {
        lower_bounds[*index] = -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
        upper_bounds[*index] = JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
    }

    lower_bounds[active_parameter_layout.log_stationary_event_rate_scale_index] = 0.0;
    upper_bounds[active_parameter_layout.log_stationary_event_rate_scale_index] = 0.0;

    let (nig_lower_bounds, nig_upper_bounds) = joint_nig_search_bounds_from_feasible_sources(
        baseline_15m_calibration_sample,
        selected_return_nig_configuration,
        return_component_basin_diagnostics,
        observed_data,
        baseline_modeled_open_event_rate_per_hour,
        solver_settings,
    )?;

    for (coordinate_position, parameter_index) in
        active_parameter_layout.nig_block.iter().enumerate()
    {
        lower_bounds[*parameter_index] = nig_lower_bounds[coordinate_position];
        upper_bounds[*parameter_index] = nig_upper_bounds[coordinate_position];
    }

    for index in &active_parameter_layout.coupling_block {
        lower_bounds[*index] = -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
        upper_bounds[*index] = JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
    }

    for index in &active_parameter_layout.return_clock_block {
        lower_bounds[*index] = -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
        upper_bounds[*index] = JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
    }

    apply_long_variance_parameter_bounds(
        active_parameter_layout,
        &mut lower_bounds,
        &mut upper_bounds,
        baseline_hawkes_kernel_parameters,
        baseline_long_variance_parameters,
        observed_data.variance_persistence_fit.as_ref(),
        !observed_data.observed_variance_covariances.is_empty(),
    )?;

    apply_option_pricing_kernel_bounds(
        active_parameter_layout,
        &mut lower_bounds,
        &mut upper_bounds,
    );

    ensure_parameter_vector_inside_bounds(
        baseline_parameter_vector,
        lower_bounds.as_slice(),
        upper_bounds.as_slice(),
    )?;

    Ok((lower_bounds, upper_bounds))
}

pub(in crate::stages::joint_refinement) fn apply_option_pricing_kernel_bounds(
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &mut [f64],
    upper_bounds: &mut [f64],
) {
    lower_bounds[active_parameter_layout.option_pricing_log_buy_activity_premium_index] =
        -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;

    upper_bounds[active_parameter_layout.option_pricing_log_buy_activity_premium_index] =
        JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;

    lower_bounds[active_parameter_layout.option_pricing_log_sell_activity_premium_index] =
        -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;

    upper_bounds[active_parameter_layout.option_pricing_log_sell_activity_premium_index] =
        JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;

    lower_bounds[active_parameter_layout
        .option_pricing_pressure_signed_imbalance_exposure_risk_price_index] =
        -JOINT_REFINEMENT_PRESSURE_RISK_PRICE_LOG_DENSITY_BOUND;

    upper_bounds[active_parameter_layout
        .option_pricing_pressure_signed_imbalance_exposure_risk_price_index] =
        JOINT_REFINEMENT_PRESSURE_RISK_PRICE_LOG_DENSITY_BOUND;
}

fn joint_nig_search_bounds_from_feasible_sources(
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    selected_return_nig_configuration: NigSharedPlusShiftConfiguration,
    return_component_basin_diagnostics: &[ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    observed_data: &JointRefinementObservedData,
    baseline_modeled_open_event_rate_per_hour: f64,
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<([f64; 4], [f64; 4]), String> {
    let (mut lower_bounds, mut upper_bounds) =
        nig_search_bounds_for_calibration_sample_with_solver_settings(
            baseline_15m_calibration_sample,
            solver_settings,
        );

    let selected_search_point = nig_search_point_from_configuration(
        baseline_15m_calibration_sample,
        selected_return_nig_configuration,
    )
    .ok_or_else(|| {
        "selected return-stage NIG cannot be projected into joint NIG coordinates".to_string()
    })?;

    widen_nig_bounds_to_include_search_point(
        &mut lower_bounds,
        &mut upper_bounds,
        selected_search_point,
    );

    for basin_search_point in return_component_basin_diagnostics
        .iter()
        .filter_map(|diagnostic| {
            project_basin_diagnostic_into_joint_nig_coordinates(
                baseline_15m_calibration_sample,
                diagnostic,
            )
        })
    {
        widen_nig_bounds_to_include_search_point(
            &mut lower_bounds,
            &mut upper_bounds,
            basin_search_point,
        );
    }

    let activity_aligned_15m_calibration_sample =
        build_continuous_calibration_sample_from_aggregated_intervals(
            observed_data.observed_15m_intervals.as_slice(),
            observed_data.observed_open_event_rate_per_hour,
            observed_data.observed_open_event_rate_per_hour,
            observed_data.fallback_sell_share_proxy,
        );

    for tick_moment_search_point in
        projected_tick_moment_nig_search_points(ProjectedTickMomentNigSearchPointsInput {
            calibration_sample: &activity_aligned_15m_calibration_sample,
            observed_data,
            baseline_modeled_open_event_rate_per_hour,
            target_modeled_open_event_rate_per_hour: observed_data
                .observed_open_event_rate_per_hour,
            solver_settings,
        })
    {
        widen_nig_bounds_to_include_search_point(
            &mut lower_bounds,
            &mut upper_bounds,
            tick_moment_search_point,
        );
    }

    widen_nig_tail_bounds_for_tick_shape(
        &mut lower_bounds,
        &mut upper_bounds,
        observed_data,
        baseline_modeled_open_event_rate_per_hour,
    );

    Ok((lower_bounds, upper_bounds))
}

fn widen_nig_tail_bounds_for_tick_shape(
    lower_bounds: &mut [f64; 4],
    upper_bounds: &mut [f64; 4],
    observed_data: &JointRefinementObservedData,
    baseline_modeled_open_event_rate_per_hour: f64,
) {
    if !baseline_modeled_open_event_rate_per_hour.is_finite()
        || baseline_modeled_open_event_rate_per_hour <= 0.0
        || !observed_data.observed_open_event_rate_per_hour.is_finite()
        || observed_data.observed_open_event_rate_per_hour <= 0.0
    {
        return;
    }

    let tick_calibration_sample = rescaled_observed_tick_calibration_sample_for_modeled_rate(
        observed_data,
        baseline_modeled_open_event_rate_per_hour,
        observed_data.observed_open_event_rate_per_hour,
    );

    let (tick_log_alpha_lower_bound, tick_log_alpha_upper_bound) =
        nig_log_alpha_bounds_for_calibration_sample(&tick_calibration_sample);

    lower_bounds[0] = lower_bounds[0].min(tick_log_alpha_lower_bound);
    upper_bounds[0] = upper_bounds[0].max(tick_log_alpha_upper_bound);
}

fn widen_nig_bounds_to_include_search_point(
    lower_bounds: &mut [f64; 4],
    upper_bounds: &mut [f64; 4],
    search_point: NigShapeSearchPoint,
) {
    let coordinates = nig_search_point_coordinates(search_point);

    for coordinate_index in 0..coordinates.len() {
        lower_bounds[coordinate_index] =
            lower_bounds[coordinate_index].min(coordinates[coordinate_index]);

        upper_bounds[coordinate_index] =
            upper_bounds[coordinate_index].max(coordinates[coordinate_index]);
    }
}

fn nig_search_point_coordinates(search_point: NigShapeSearchPoint) -> [f64; 4] {
    [
        search_point.log_alpha,
        search_point.buy_skew_coordinate,
        search_point.sell_relative_coordinate,
        search_point.log_delta_offset,
    ]
}

fn ensure_parameter_vector_inside_bounds(
    parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Result<(), String> {
    if parameter_vector.len() != lower_bounds.len() || parameter_vector.len() != upper_bounds.len()
    {
        return Err(format!(
            "joint refinement parameter bound length mismatch vector={} lower={} upper={}",
            parameter_vector.len(),
            lower_bounds.len(),
            upper_bounds.len()
        ));
    }

    for (parameter_index, (value, (lower_bound, upper_bound))) in parameter_vector
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .enumerate()
    {
        if value < lower_bound || value > upper_bound {
            return Err(format!(
                "joint refinement baseline parameter is outside bounds parameter_index={parameter_index} value={value} lower_bound={lower_bound} upper_bound={upper_bound}"
            ));
        }
    }

    Ok(())
}
