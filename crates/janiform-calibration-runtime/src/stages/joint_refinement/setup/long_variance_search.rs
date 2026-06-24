use janiform_foundations::numeric::clamp_point_in_place as clamp_in_place;
use janiform_model_kernels::hawkes::HawkesKernelParameters;
use janiform_model_kernels::long_variance::LongVarianceParameters;

use super::super::execution::{
    JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND, JointRefinementActiveParameterLayout,
};
use crate::stages::long_variance::policy::{
    long_variance_mean_reversion_rate_calibration_upper_bound,
    long_variance_side_event_feedback_drift_budget,
    long_variance_stationary_event_rates_for_hawkes_kernel,
    long_variance_vol_of_vol_upper_bound_for_mean_reversion_rate,
};

pub(super) fn apply_long_variance_parameter_bounds(
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &mut [f64],
    upper_bounds: &mut [f64],
    baseline_hawkes_kernel_parameters: HawkesKernelParameters,
    baseline_long_variance_parameters: LongVarianceParameters,
) -> Result<(), String> {
    baseline_long_variance_parameters
        .validate()
        .map_err(|source| format!("invalid baseline long variance parameters: {source}"))?;

    let log_mean_reversion_rate_upper_bound =
        long_variance_log_mean_reversion_rate_upper_bound(baseline_long_variance_parameters)?;

    lower_bounds[active_parameter_layout.log_long_variance_mean_reversion_rate_scale_index] =
        -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
    upper_bounds[active_parameter_layout.log_long_variance_mean_reversion_rate_scale_index] =
        log_mean_reversion_rate_upper_bound;

    lower_bounds[active_parameter_layout.long_variance_vol_of_vol_index] = 0.0;
    upper_bounds[active_parameter_layout.long_variance_vol_of_vol_index] =
        long_variance_vol_of_vol_upper_bound(baseline_long_variance_parameters)?;

    lower_bounds[active_parameter_layout.long_variance_return_energy_loading_index] = 0.0;
    upper_bounds[active_parameter_layout.long_variance_return_energy_loading_index] =
        long_variance_return_energy_loading_upper_bound(baseline_long_variance_parameters)?
            .max(baseline_long_variance_parameters.return_energy_loading);

    let (buy_event_loading_upper_bound, sell_event_loading_upper_bound) =
        long_variance_event_loading_upper_bounds(
            baseline_hawkes_kernel_parameters,
            baseline_long_variance_parameters,
        )?;

    lower_bounds[active_parameter_layout.long_variance_buy_event_loading_index] = 0.0;
    upper_bounds[active_parameter_layout.long_variance_buy_event_loading_index] =
        buy_event_loading_upper_bound.max(baseline_long_variance_parameters.buy_event_loading);

    lower_bounds[active_parameter_layout.long_variance_sell_event_loading_index] = 0.0;
    upper_bounds[active_parameter_layout.long_variance_sell_event_loading_index] =
        sell_event_loading_upper_bound.max(baseline_long_variance_parameters.sell_event_loading);

    Ok(())
}

fn long_variance_return_energy_loading_upper_bound(
    baseline_long_variance_parameters: LongVarianceParameters,
) -> Result<f64, String> {
    let maximum_mean_reversion_rate =
        long_variance_maximum_calibration_mean_reversion_rate(baseline_long_variance_parameters)?;

    let feedback_drift_budget =
        long_variance_side_event_feedback_drift_budget(maximum_mean_reversion_rate).ok_or_else(
            || {
                format!(
                    "invalid long variance return-energy drift budget maximum_mean_reversion_rate={maximum_mean_reversion_rate}"
                )
            },
        )?;

    long_variance_feedback_loading_upper_bound(
        feedback_drift_budget,
        baseline_long_variance_parameters.stationary_return_energy_rate_per_hour,
        "stationary_return_energy_rate_per_hour",
    )
}

fn long_variance_vol_of_vol_upper_bound(
    baseline_long_variance_parameters: LongVarianceParameters,
) -> Result<f64, String> {
    let maximum_mean_reversion_rate =
        long_variance_maximum_calibration_mean_reversion_rate(baseline_long_variance_parameters)?;

    let upper_bound =
        long_variance_vol_of_vol_upper_bound_for_mean_reversion_rate(maximum_mean_reversion_rate)
            .ok_or_else(|| {
                format!(
                    "invalid long variance mean-reversion bound maximum_mean_reversion_rate={maximum_mean_reversion_rate}"
                )
            })?;

    if !upper_bound.is_finite() || upper_bound < baseline_long_variance_parameters.vol_of_vol {
        return Err(format!(
            "invalid long variance vol-of-vol bound upper_bound={upper_bound} baseline_vol_of_vol={}",
            baseline_long_variance_parameters.vol_of_vol
        ));
    }

    Ok(upper_bound)
}

fn long_variance_log_mean_reversion_rate_upper_bound(
    baseline_long_variance_parameters: LongVarianceParameters,
) -> Result<f64, String> {
    let maximum_mean_reversion_rate =
        long_variance_maximum_calibration_mean_reversion_rate(baseline_long_variance_parameters)?;

    let upper_bound =
        (maximum_mean_reversion_rate / baseline_long_variance_parameters.mean_reversion_rate).ln();
    if !upper_bound.is_finite() || upper_bound < 0.0 {
        return Err(format!(
            "invalid long variance mean-reversion log bound upper_bound={upper_bound}"
        ));
    }

    Ok(upper_bound)
}

fn long_variance_maximum_calibration_mean_reversion_rate(
    baseline_long_variance_parameters: LongVarianceParameters,
) -> Result<f64, String> {
    let multiplicative_upper_bound = baseline_long_variance_parameters.mean_reversion_rate
        * JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND.exp();

    let calibration_upper_bound = long_variance_mean_reversion_rate_calibration_upper_bound();
    let maximum_mean_reversion_rate = multiplicative_upper_bound.min(calibration_upper_bound);

    if !maximum_mean_reversion_rate.is_finite()
        || maximum_mean_reversion_rate < baseline_long_variance_parameters.mean_reversion_rate
    {
        return Err(format!(
            "invalid long variance mean-reversion calibration bound maximum_mean_reversion_rate={maximum_mean_reversion_rate} baseline_mean_reversion_rate={}",
            baseline_long_variance_parameters.mean_reversion_rate
        ));
    }

    Ok(maximum_mean_reversion_rate)
}

fn long_variance_event_loading_upper_bounds(
    baseline_hawkes_kernel_parameters: HawkesKernelParameters,
    baseline_long_variance_parameters: LongVarianceParameters,
) -> Result<(f64, f64), String> {
    let stationary_event_rates =
        long_variance_stationary_event_rates_for_hawkes_kernel(baseline_hawkes_kernel_parameters)
            .ok_or_else(|| {
            "long variance bounds require finite stationary Hawkes event rates".to_string()
        })?;

    let maximum_mean_reversion_rate =
        long_variance_maximum_calibration_mean_reversion_rate(baseline_long_variance_parameters)?;

    let side_event_drift_budget = long_variance_side_event_feedback_drift_budget(
        maximum_mean_reversion_rate,
    )
    .ok_or_else(|| {
        format!(
            "invalid long variance side event-loading drift budget maximum_mean_reversion_rate={maximum_mean_reversion_rate}"
        )
    })?;

    Ok((
        long_variance_feedback_loading_upper_bound(
            side_event_drift_budget,
            stationary_event_rates.buy_event_rate_per_hour,
            "stationary_buy_event_rate",
        )?,
        long_variance_feedback_loading_upper_bound(
            side_event_drift_budget,
            stationary_event_rates.sell_event_rate_per_hour,
            "stationary_sell_event_rate",
        )?,
    ))
}

fn long_variance_feedback_loading_upper_bound(
    feedback_drift_budget: f64,
    stationary_feedback_rate: f64,
    stationary_feedback_rate_label: &str,
) -> Result<f64, String> {
    if !feedback_drift_budget.is_finite() || feedback_drift_budget <= 0.0 {
        return Err(format!(
            "long variance feedback-loading drift budget must be finite positive, found {feedback_drift_budget}"
        ));
    }

    if !stationary_feedback_rate.is_finite() || stationary_feedback_rate < 0.0 {
        return Err(format!(
            "{stationary_feedback_rate_label} must be finite non-negative, found {stationary_feedback_rate}"
        ));
    }

    if stationary_feedback_rate == 0.0 {
        return Ok(0.0);
    }

    let upper_bound = feedback_drift_budget / stationary_feedback_rate;
    if !upper_bound.is_finite() {
        return Err(format!(
            "long variance feedback-loading bound must be finite for {stationary_feedback_rate_label}={stationary_feedback_rate}"
        ));
    }

    Ok(upper_bound)
}

pub(super) fn joint_long_variance_candidate_seeds(
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<(String, Vec<f64>)> {
    let single_coordinate_seeds = [
        (
            "long_variance_mean_reversion_up",
            active_parameter_layout.log_long_variance_mean_reversion_rate_scale_index,
            LongVarianceCandidateSeedDirection::Upper,
        ),
        (
            "long_variance_mean_reversion_down",
            active_parameter_layout.log_long_variance_mean_reversion_rate_scale_index,
            LongVarianceCandidateSeedDirection::Lower,
        ),
        (
            "long_variance_vol_of_vol_up",
            active_parameter_layout.long_variance_vol_of_vol_index,
            LongVarianceCandidateSeedDirection::Upper,
        ),
        (
            "long_variance_return_energy_loading_up",
            active_parameter_layout.long_variance_return_energy_loading_index,
            LongVarianceCandidateSeedDirection::Upper,
        ),
        (
            "long_variance_buy_event_loading_up",
            active_parameter_layout.long_variance_buy_event_loading_index,
            LongVarianceCandidateSeedDirection::Upper,
        ),
        (
            "long_variance_sell_event_loading_up",
            active_parameter_layout.long_variance_sell_event_loading_index,
            LongVarianceCandidateSeedDirection::Upper,
        ),
    ];

    let mut seeds = single_coordinate_seeds
        .into_iter()
        .map(|(seed_id, parameter_index, direction)| {
            (
                seed_id.to_string(),
                single_long_variance_candidate_seed(
                    baseline_parameter_vector,
                    lower_bounds,
                    upper_bounds,
                    parameter_index,
                    direction,
                ),
            )
        })
        .collect::<Vec<_>>();

    let mut symmetric_event_loading_seed = baseline_parameter_vector.to_vec();

    set_coordinate_midpoint_toward_bound(
        symmetric_event_loading_seed.as_mut_slice(),
        lower_bounds,
        upper_bounds,
        active_parameter_layout.long_variance_return_energy_loading_index,
        LongVarianceCandidateSeedDirection::Upper,
    );

    set_coordinate_midpoint_toward_bound(
        symmetric_event_loading_seed.as_mut_slice(),
        lower_bounds,
        upper_bounds,
        active_parameter_layout.long_variance_buy_event_loading_index,
        LongVarianceCandidateSeedDirection::Upper,
    );

    set_coordinate_midpoint_toward_bound(
        symmetric_event_loading_seed.as_mut_slice(),
        lower_bounds,
        upper_bounds,
        active_parameter_layout.long_variance_sell_event_loading_index,
        LongVarianceCandidateSeedDirection::Upper,
    );

    clamp_in_place(
        symmetric_event_loading_seed.as_mut_slice(),
        lower_bounds,
        upper_bounds,
    );

    seeds.push((
        "long_variance_feedback_loading_symmetric_up".to_string(),
        symmetric_event_loading_seed,
    ));

    seeds
}

#[derive(Debug, Clone, Copy)]
enum LongVarianceCandidateSeedDirection {
    Lower,
    Upper,
}

fn single_long_variance_candidate_seed(
    baseline_parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    parameter_index: usize,
    direction: LongVarianceCandidateSeedDirection,
) -> Vec<f64> {
    let mut candidate_seed = baseline_parameter_vector.to_vec();

    set_coordinate_midpoint_toward_bound(
        candidate_seed.as_mut_slice(),
        lower_bounds,
        upper_bounds,
        parameter_index,
        direction,
    );

    clamp_in_place(candidate_seed.as_mut_slice(), lower_bounds, upper_bounds);

    candidate_seed
}

fn set_coordinate_midpoint_toward_bound(
    parameter_vector: &mut [f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    parameter_index: usize,
    direction: LongVarianceCandidateSeedDirection,
) {
    let Some(current_value) = parameter_vector.get_mut(parameter_index) else {
        return;
    };

    let Some(lower_bound) = lower_bounds.get(parameter_index) else {
        return;
    };

    let Some(upper_bound) = upper_bounds.get(parameter_index) else {
        return;
    };

    let target_bound = match direction {
        LongVarianceCandidateSeedDirection::Lower => *lower_bound,
        LongVarianceCandidateSeedDirection::Upper => *upper_bound,
    };

    *current_value = 0.5 * (*current_value + target_bound);
}

#[cfg(test)]
mod tests {
    use janiform_model_kernels::hawkes::{HawkesKernelLoadings, HawkesKernelParameters};
    use janiform_model_kernels::long_variance::LongVarianceParameters;

    use super::super::test_support::test_active_parameter_layout;
    use super::{apply_long_variance_parameter_bounds, joint_long_variance_candidate_seeds};

    fn test_hawkes_kernel_parameters() -> HawkesKernelParameters {
        HawkesKernelParameters::try_new(
            1.0,
            2.0,
            HawkesKernelLoadings {
                plus_plus: 0.2,
                plus_minus: 0.1,
                minus_plus: 0.1,
                minus_minus: 0.2,
            },
            HawkesKernelLoadings {
                plus_plus: 0.05,
                plus_minus: 0.02,
                minus_plus: 0.02,
                minus_minus: 0.05,
            },
        )
        .expect("test Hawkes parameters should validate")
    }

    #[test]
    fn long_variance_bounds_keep_symmetric_event_loading_corner_stationary() {
        let active_parameter_layout = test_active_parameter_layout();
        let baseline_long_variance_parameters = LongVarianceParameters::neutral();

        let mut lower_bounds = vec![0.0; active_parameter_layout.vector_length()];
        let mut upper_bounds = vec![0.0; active_parameter_layout.vector_length()];

        apply_long_variance_parameter_bounds(
            &active_parameter_layout,
            lower_bounds.as_mut_slice(),
            upper_bounds.as_mut_slice(),
            test_hawkes_kernel_parameters(),
            baseline_long_variance_parameters,
        )
        .expect("long variance bounds should build");

        let mean_reversion_rate_upper_bound = baseline_long_variance_parameters.mean_reversion_rate
            * upper_bounds
                [active_parameter_layout.log_long_variance_mean_reversion_rate_scale_index]
                .exp();

        let corner_parameters = LongVarianceParameters::try_new(
            mean_reversion_rate_upper_bound,
            0.0,
            baseline_long_variance_parameters.stationary_return_energy_rate_per_hour,
            upper_bounds[active_parameter_layout.long_variance_return_energy_loading_index],
            upper_bounds[active_parameter_layout.long_variance_buy_event_loading_index],
            upper_bounds[active_parameter_layout.long_variance_sell_event_loading_index],
        )
        .expect("upper event-loading corner should satisfy primitive parameter constraints");

        let stationary_level = corner_parameters
            .stationary_mean_reversion_level_for_hawkes_kernel(
                test_hawkes_kernel_parameters(),
                janiform_model_kernels::hawkes::HawkesStationaryTargetSourceRates::zero(),
            )
            .expect("upper event-loading corner should keep positive stationary level");

        assert!(stationary_level > 0.0);
    }

    #[test]
    fn long_variance_candidate_seeds_move_each_active_coordinate() {
        let active_parameter_layout = test_active_parameter_layout();
        let baseline_parameter_vector = vec![0.0; active_parameter_layout.vector_length()];

        let lower_bounds = vec![-1.0; active_parameter_layout.vector_length()];
        let upper_bounds = vec![2.0; active_parameter_layout.vector_length()];

        let candidate_seeds = joint_long_variance_candidate_seeds(
            baseline_parameter_vector.as_slice(),
            &active_parameter_layout,
            lower_bounds.as_slice(),
            upper_bounds.as_slice(),
        );

        let candidate_seed_ids = candidate_seeds
            .iter()
            .map(|(candidate_id, _)| candidate_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            candidate_seed_ids,
            vec![
                "long_variance_mean_reversion_up",
                "long_variance_mean_reversion_down",
                "long_variance_vol_of_vol_up",
                "long_variance_return_energy_loading_up",
                "long_variance_buy_event_loading_up",
                "long_variance_sell_event_loading_up",
                "long_variance_feedback_loading_symmetric_up",
            ]
        );

        let vol_of_vol_seed = candidate_seeds
            .iter()
            .find(|(candidate_id, _)| candidate_id == "long_variance_vol_of_vol_up")
            .map(|(_, candidate_seed)| candidate_seed)
            .expect("vol-of-vol seed should be present");

        assert_eq!(
            vol_of_vol_seed[active_parameter_layout.long_variance_vol_of_vol_index],
            1.0
        );
    }
}
