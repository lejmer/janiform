use janiform_foundations::numeric::clamp_point_in_place as clamp_in_place;
use janiform_model_kernels::hawkes::stationary_intensity_multipliers;
use janiform_model_kernels::return_clock::{
    ReturnClockExposureParameters, stationary_total_return_clock,
};

use super::super::execution::{JointRefinementActiveParameterLayout, JointRefinementContext};
use super::core::apply_nig_log_delta_offset_shift;

#[derive(Debug, Clone, Copy)]
pub(super) struct ReturnClockExposureCandidateSeedProfile {
    baseline_buy_excitation_return_clock_scale: f64,
    baseline_sell_excitation_return_clock_scale: f64,
    stationary_buy_intensity_multiplier: f64,
    stationary_sell_intensity_multiplier: f64,
}

pub(super) struct JointReturnClockExposureSeedsBuildInput<'a> {
    pub(super) baseline_parameter_vector: &'a [f64],
    pub(super) active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    pub(super) lower_bounds: &'a [f64],
    pub(super) upper_bounds: &'a [f64],
    pub(super) return_clock_seed_profile: ReturnClockExposureCandidateSeedProfile,
}

pub(super) fn joint_return_clock_exposure_candidate_seeds(
    input: JointReturnClockExposureSeedsBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let JointReturnClockExposureSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        return_clock_seed_profile,
    } = input;

    [
        (
            "return_clock_buy_excitation_up",
            active_parameter_layout.log_buy_excitation_return_clock_scale_index,
            1.0,
            true,
        ),
        (
            "return_clock_buy_excitation_down",
            active_parameter_layout.log_buy_excitation_return_clock_scale_index,
            -1.0,
            true,
        ),
        (
            "return_clock_sell_excitation_up",
            active_parameter_layout.log_sell_excitation_return_clock_scale_index,
            1.0,
            false,
        ),
        (
            "return_clock_sell_excitation_down",
            active_parameter_layout.log_sell_excitation_return_clock_scale_index,
            -1.0,
            false,
        ),
    ]
    .into_iter()
    .filter_map(
        |(seed_id, return_clock_parameter_index, direction, move_buy_side)| {
            let log_scale_shift = midpoint_log_scale_shift_toward_bound(
                return_clock_parameter_index,
                baseline_parameter_vector,
                lower_bounds,
                upper_bounds,
                direction,
            )?;

            let buy_log_scale_shift = if move_buy_side { log_scale_shift } else { 0.0 };

            let sell_log_scale_shift = if move_buy_side { 0.0 } else { log_scale_shift };

            build_return_clock_exposure_candidate_seed(
                ReturnClockExposureCandidateSeedInput {
                    seed_id,
                    baseline_parameter_vector,
                    active_parameter_layout,
                    lower_bounds,
                    upper_bounds,
                    return_clock_seed_profile,
                },
                buy_log_scale_shift,
                sell_log_scale_shift,
            )
        },
    )
    .collect()
}

pub(super) fn return_clock_exposure_candidate_seed_profile(
    joint_context: &JointRefinementContext,
) -> Option<ReturnClockExposureCandidateSeedProfile> {
    let stationary_multipliers = stationary_intensity_multipliers(
        joint_context
            .baseline_hawkes_fitted_parameters
            .kernel_parameters
            .alpha,
        joint_context
            .baseline_hawkes_fitted_parameters
            .kernel_parameters
            .beta,
    )?;

    let baseline_return_clock_exposure = joint_context
        .baseline_return_components_fitted_parameters
        .return_clock_exposure_parameters;

    let profile = ReturnClockExposureCandidateSeedProfile {
        baseline_buy_excitation_return_clock_scale: baseline_return_clock_exposure
            .buy_excitation_return_clock_scale,
        baseline_sell_excitation_return_clock_scale: baseline_return_clock_exposure
            .sell_excitation_return_clock_scale,
        stationary_buy_intensity_multiplier: stationary_multipliers.buy_intensity_multiplier,
        stationary_sell_intensity_multiplier: stationary_multipliers.sell_intensity_multiplier,
    };

    return_clock_exposure_candidate_seed_profile_is_valid(profile).then_some(profile)
}

struct ReturnClockExposureCandidateSeedInput<'a> {
    seed_id: &'a str,
    baseline_parameter_vector: &'a [f64],
    active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    return_clock_seed_profile: ReturnClockExposureCandidateSeedProfile,
}

fn build_return_clock_exposure_candidate_seed(
    input: ReturnClockExposureCandidateSeedInput<'_>,
    buy_log_scale_shift: f64,
    sell_log_scale_shift: f64,
) -> Option<(String, Vec<f64>)> {
    let buy_index = input
        .active_parameter_layout
        .log_buy_excitation_return_clock_scale_index;

    let sell_index = input
        .active_parameter_layout
        .log_sell_excitation_return_clock_scale_index;

    let baseline_buy_log_scale = *input.baseline_parameter_vector.get(buy_index)?;
    let baseline_sell_log_scale = *input.baseline_parameter_vector.get(sell_index)?;

    let mut candidate_seed = input.baseline_parameter_vector.to_vec();

    *candidate_seed.get_mut(buy_index)? += buy_log_scale_shift;
    *candidate_seed.get_mut(sell_index)? += sell_log_scale_shift;

    clamp_in_place(
        candidate_seed.as_mut_slice(),
        input.lower_bounds,
        input.upper_bounds,
    );

    let candidate_buy_log_scale = *candidate_seed.get(buy_index)?;
    let candidate_sell_log_scale = *candidate_seed.get(sell_index)?;

    if candidate_buy_log_scale == baseline_buy_log_scale
        && candidate_sell_log_scale == baseline_sell_log_scale
    {
        return None;
    }

    let log_delta_offset_shift = stationary_return_clock_log_delta_compensation(
        input.return_clock_seed_profile,
        baseline_buy_log_scale,
        baseline_sell_log_scale,
        candidate_buy_log_scale,
        candidate_sell_log_scale,
    )?;

    apply_nig_log_delta_offset_shift(
        candidate_seed.as_mut_slice(),
        input.active_parameter_layout,
        log_delta_offset_shift,
    );

    clamp_in_place(
        candidate_seed.as_mut_slice(),
        input.lower_bounds,
        input.upper_bounds,
    );

    Some((input.seed_id.to_string(), candidate_seed))
}

fn return_clock_exposure_candidate_seed_profile_is_valid(
    profile: ReturnClockExposureCandidateSeedProfile,
) -> bool {
    [
        profile.baseline_buy_excitation_return_clock_scale,
        profile.baseline_sell_excitation_return_clock_scale,
        profile.stationary_buy_intensity_multiplier,
        profile.stationary_sell_intensity_multiplier,
    ]
    .into_iter()
    .all(|value| value.is_finite() && value > 0.0)
        && profile.stationary_buy_intensity_multiplier >= 1.0
        && profile.stationary_sell_intensity_multiplier >= 1.0
}

fn stationary_return_clock_log_delta_compensation(
    profile: ReturnClockExposureCandidateSeedProfile,
    baseline_buy_log_scale: f64,
    baseline_sell_log_scale: f64,
    candidate_buy_log_scale: f64,
    candidate_sell_log_scale: f64,
) -> Option<f64> {
    let baseline_total_return_clock = stationary_total_return_clock_for_exposure(
        profile,
        baseline_buy_log_scale,
        baseline_sell_log_scale,
    )?;

    let candidate_total_return_clock = stationary_total_return_clock_for_exposure(
        profile,
        candidate_buy_log_scale,
        candidate_sell_log_scale,
    )?;

    let log_delta_offset_shift = (baseline_total_return_clock / candidate_total_return_clock).ln();

    log_delta_offset_shift
        .is_finite()
        .then_some(log_delta_offset_shift)
}

fn stationary_total_return_clock_for_exposure(
    profile: ReturnClockExposureCandidateSeedProfile,
    buy_log_scale: f64,
    sell_log_scale: f64,
) -> Option<f64> {
    if !return_clock_exposure_candidate_seed_profile_is_valid(profile)
        || !buy_log_scale.is_finite()
        || !sell_log_scale.is_finite()
    {
        return None;
    }

    let exposure_parameters = ReturnClockExposureParameters::try_new(
        profile.baseline_buy_excitation_return_clock_scale * buy_log_scale.exp(),
        profile.baseline_sell_excitation_return_clock_scale * sell_log_scale.exp(),
    )
    .ok()?;

    stationary_total_return_clock(
        exposure_parameters,
        profile.stationary_buy_intensity_multiplier,
        profile.stationary_sell_intensity_multiplier,
    )
}

fn midpoint_log_scale_shift_toward_bound(
    parameter_index: usize,
    baseline_parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    direction: f64,
) -> Option<f64> {
    if baseline_parameter_vector.len() != lower_bounds.len()
        || baseline_parameter_vector.len() != upper_bounds.len()
    {
        return None;
    }

    let current_value = *baseline_parameter_vector.get(parameter_index)?;
    let lower_bound = *lower_bounds.get(parameter_index)?;
    let upper_bound = *upper_bounds.get(parameter_index)?;
    let target_bound = if direction < 0.0 {
        lower_bound
    } else {
        upper_bound
    };
    let shifted_value = 0.5 * (current_value + target_bound);
    let shift = shifted_value - current_value;

    (shift.is_finite() && shift != 0.0).then_some(shift)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::test_active_parameter_layout;
    use super::{
        JointReturnClockExposureSeedsBuildInput, ReturnClockExposureCandidateSeedProfile,
        joint_return_clock_exposure_candidate_seeds, midpoint_log_scale_shift_toward_bound,
        stationary_return_clock_log_delta_compensation,
    };

    fn test_return_clock_seed_profile() -> ReturnClockExposureCandidateSeedProfile {
        ReturnClockExposureCandidateSeedProfile {
            baseline_buy_excitation_return_clock_scale: 1.0,
            baseline_sell_excitation_return_clock_scale: 1.0,
            stationary_buy_intensity_multiplier: 3.0,
            stationary_sell_intensity_multiplier: 2.0,
        }
    }

    #[test]
    fn return_clock_log_delta_compensation_preserves_stationary_return_clock_scale() {
        let log_delta_shift = stationary_return_clock_log_delta_compensation(
            test_return_clock_seed_profile(),
            0.0,
            0.0,
            2.0f64.ln(),
            0.0,
        )
        .expect("finite stationary compensation should be available");

        assert_eq!(log_delta_shift, (5.0f64 / 7.0).ln());
    }

    #[test]
    fn return_clock_exposure_candidate_seeds_move_exposure_and_compensate_delta() {
        let active_parameter_layout = test_active_parameter_layout();
        let baseline_parameter_vector = vec![0.0; active_parameter_layout.vector_length()];

        let lower_bounds = vec![-2.0; baseline_parameter_vector.len()];
        let upper_bounds = vec![2.0; baseline_parameter_vector.len()];

        let candidate_seeds =
            joint_return_clock_exposure_candidate_seeds(JointReturnClockExposureSeedsBuildInput {
                baseline_parameter_vector: baseline_parameter_vector.as_slice(),
                active_parameter_layout: &active_parameter_layout,
                lower_bounds: lower_bounds.as_slice(),
                upper_bounds: upper_bounds.as_slice(),
                return_clock_seed_profile: test_return_clock_seed_profile(),
            });

        let candidate_seed_ids = candidate_seeds
            .iter()
            .map(|(candidate_id, _)| candidate_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            candidate_seed_ids,
            vec![
                "return_clock_buy_excitation_up",
                "return_clock_buy_excitation_down",
                "return_clock_sell_excitation_up",
                "return_clock_sell_excitation_down",
            ]
        );

        let buy_log_scale_shift = midpoint_log_scale_shift_toward_bound(
            active_parameter_layout.log_buy_excitation_return_clock_scale_index,
            baseline_parameter_vector.as_slice(),
            lower_bounds.as_slice(),
            upper_bounds.as_slice(),
            1.0,
        )
        .expect("test bounds have positive width");

        let buy_up_seed = candidate_seeds
            .iter()
            .find(|(candidate_id, _)| candidate_id == "return_clock_buy_excitation_up")
            .map(|(_, candidate_seed)| candidate_seed.as_slice())
            .expect("buy-up return-clock seed should be present");

        let expected_log_delta_shift = stationary_return_clock_log_delta_compensation(
            test_return_clock_seed_profile(),
            0.0,
            0.0,
            buy_log_scale_shift,
            0.0,
        )
        .expect("finite stationary compensation should be available");

        assert_eq!(
            buy_up_seed[active_parameter_layout.log_buy_excitation_return_clock_scale_index],
            buy_log_scale_shift
        );
        assert_eq!(
            buy_up_seed[active_parameter_layout.log_sell_excitation_return_clock_scale_index],
            0.0
        );
        assert_eq!(
            buy_up_seed[active_parameter_layout.nig_block[3]],
            expected_log_delta_shift
        );
    }

    #[test]
    fn return_clock_seed_shift_moves_halfway_toward_bound() {
        let baseline_parameter_vector = vec![0.0, 0.0];
        let lower_bounds = vec![-2.0, -2.0];
        let upper_bounds = vec![2.0, 2.0];

        let upward_shift = midpoint_log_scale_shift_toward_bound(
            0,
            baseline_parameter_vector.as_slice(),
            lower_bounds.as_slice(),
            upper_bounds.as_slice(),
            1.0,
        )
        .expect("upward shift should exist");

        let downward_shift = midpoint_log_scale_shift_toward_bound(
            0,
            baseline_parameter_vector.as_slice(),
            lower_bounds.as_slice(),
            upper_bounds.as_slice(),
            -1.0,
        )
        .expect("downward shift should exist");

        assert_eq!(upward_shift, 1.0);
        assert_eq!(downward_shift, -1.0);
    }
}
