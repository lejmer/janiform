use janiform_foundations::numeric::clamp_point_in_place as clamp_in_place;
use janiform_model_kernels::hawkes::stationary_intensity_multipliers;
use janiform_model_kernels::return_clock::{
    ReturnClockExposureParameters, stationary_total_return_clock,
};

use crate::stages::adaptive_direct_search::initial_mesh_radii;

use super::super::execution::{
    JointRefinementActiveParameterLayout, JointRefinementContext,
    joint_refinement_restart_full_vector_initial_mesh_divisor,
};
use super::core::apply_nig_log_delta_offset_shift;

#[derive(Debug, Clone, Copy)]
pub(super) struct ReturnClockExposureRestartSeedProfile {
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
    pub(super) return_clock_seed_profile: ReturnClockExposureRestartSeedProfile,
}

pub(super) fn joint_return_clock_exposure_restart_seeds(
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
            let mesh_radius = initial_restart_mesh_radius_for_coordinate(
                return_clock_parameter_index,
                baseline_parameter_vector,
                lower_bounds,
                upper_bounds,
            )?;

            let buy_log_scale_shift = move_buy_side
                .then_some(direction * mesh_radius)
                .unwrap_or(0.0);

            let sell_log_scale_shift = (!move_buy_side)
                .then_some(direction * mesh_radius)
                .unwrap_or(0.0);

            build_return_clock_exposure_restart_seed(
                seed_id,
                baseline_parameter_vector,
                active_parameter_layout,
                lower_bounds,
                upper_bounds,
                return_clock_seed_profile,
                buy_log_scale_shift,
                sell_log_scale_shift,
            )
        },
    )
    .collect()
}

pub(super) fn return_clock_exposure_restart_seed_profile(
    joint_context: &JointRefinementContext,
) -> Option<ReturnClockExposureRestartSeedProfile> {
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

    let profile = ReturnClockExposureRestartSeedProfile {
        baseline_buy_excitation_return_clock_scale: baseline_return_clock_exposure
            .buy_excitation_return_clock_scale,
        baseline_sell_excitation_return_clock_scale: baseline_return_clock_exposure
            .sell_excitation_return_clock_scale,
        stationary_buy_intensity_multiplier: stationary_multipliers.buy_intensity_multiplier,
        stationary_sell_intensity_multiplier: stationary_multipliers.sell_intensity_multiplier,
    };

    return_clock_exposure_restart_seed_profile_is_valid(profile).then_some(profile)
}

fn build_return_clock_exposure_restart_seed(
    seed_id: &str,
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    return_clock_seed_profile: ReturnClockExposureRestartSeedProfile,
    buy_log_scale_shift: f64,
    sell_log_scale_shift: f64,
) -> Option<(String, Vec<f64>)> {
    let buy_index = active_parameter_layout.log_buy_excitation_return_clock_scale_index;
    let sell_index = active_parameter_layout.log_sell_excitation_return_clock_scale_index;

    let baseline_buy_log_scale = *baseline_parameter_vector.get(buy_index)?;
    let baseline_sell_log_scale = *baseline_parameter_vector.get(sell_index)?;

    let mut restart_seed = baseline_parameter_vector.to_vec();

    *restart_seed.get_mut(buy_index)? += buy_log_scale_shift;
    *restart_seed.get_mut(sell_index)? += sell_log_scale_shift;

    clamp_in_place(restart_seed.as_mut_slice(), lower_bounds, upper_bounds);

    let candidate_buy_log_scale = *restart_seed.get(buy_index)?;
    let candidate_sell_log_scale = *restart_seed.get(sell_index)?;

    if candidate_buy_log_scale == baseline_buy_log_scale
        && candidate_sell_log_scale == baseline_sell_log_scale
    {
        return None;
    }

    let log_delta_offset_shift = stationary_return_clock_log_delta_compensation(
        return_clock_seed_profile,
        baseline_buy_log_scale,
        baseline_sell_log_scale,
        candidate_buy_log_scale,
        candidate_sell_log_scale,
    )?;

    apply_nig_log_delta_offset_shift(
        restart_seed.as_mut_slice(),
        active_parameter_layout,
        log_delta_offset_shift,
    );

    clamp_in_place(restart_seed.as_mut_slice(), lower_bounds, upper_bounds);

    Some((seed_id.to_string(), restart_seed))
}

fn return_clock_exposure_restart_seed_profile_is_valid(
    profile: ReturnClockExposureRestartSeedProfile,
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
    profile: ReturnClockExposureRestartSeedProfile,
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
    profile: ReturnClockExposureRestartSeedProfile,
    buy_log_scale: f64,
    sell_log_scale: f64,
) -> Option<f64> {
    if !return_clock_exposure_restart_seed_profile_is_valid(profile)
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

fn initial_restart_mesh_radius_for_coordinate(
    parameter_index: usize,
    baseline_parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Option<f64> {
    if baseline_parameter_vector.len() != lower_bounds.len()
        || baseline_parameter_vector.len() != upper_bounds.len()
    {
        return None;
    }

    let mesh_radius = *initial_mesh_radii(
        lower_bounds,
        upper_bounds,
        joint_refinement_restart_full_vector_initial_mesh_divisor(baseline_parameter_vector.len()),
        0.0,
    )
    .get(parameter_index)?;

    (mesh_radius.is_finite() && mesh_radius > 0.0).then_some(mesh_radius)
}

#[cfg(test)]
mod tests {
    use super::{
        JointReturnClockExposureSeedsBuildInput, ReturnClockExposureRestartSeedProfile,
        initial_restart_mesh_radius_for_coordinate, joint_return_clock_exposure_restart_seeds,
        stationary_return_clock_log_delta_compensation,
    };
    use crate::stages::joint_refinement::execution::JointRefinementActiveParameterLayout;

    fn test_active_parameter_layout() -> JointRefinementActiveParameterLayout {
        JointRefinementActiveParameterLayout {
            hawkes_block: vec![0, 1, 2, 3],
            nig_block: vec![4, 5, 6, 7],
            return_clock_block: vec![8, 9],
            option_pricing_kernel_block: vec![10, 11],
            coupling_block: Vec::new(),
            log_stationary_event_rate_scale_index: 0,
            log_beta_scale_index: 1,
            log_self_excitation_scale_index: 2,
            log_cross_excitation_scale_index: 3,
            log_buy_excitation_return_clock_scale_index: 8,
            log_sell_excitation_return_clock_scale_index: 9,
            option_pricing_log_buy_activity_premium_index: 10,
            option_pricing_log_sell_activity_premium_index: 11,
            known_activity_scale_index: None,
            surprise_activity_scale_index: None,
            surprise_baseline_scale_index: None,
        }
    }

    fn test_return_clock_seed_profile() -> ReturnClockExposureRestartSeedProfile {
        ReturnClockExposureRestartSeedProfile {
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
    fn return_clock_exposure_restart_seeds_move_exposure_and_compensate_delta() {
        let active_parameter_layout = test_active_parameter_layout();
        let baseline_parameter_vector = vec![0.0; active_parameter_layout.vector_length()];

        let lower_bounds = vec![-2.0; baseline_parameter_vector.len()];
        let upper_bounds = vec![2.0; baseline_parameter_vector.len()];

        let restart_seeds =
            joint_return_clock_exposure_restart_seeds(JointReturnClockExposureSeedsBuildInput {
                baseline_parameter_vector: baseline_parameter_vector.as_slice(),
                active_parameter_layout: &active_parameter_layout,
                lower_bounds: lower_bounds.as_slice(),
                upper_bounds: upper_bounds.as_slice(),
                return_clock_seed_profile: test_return_clock_seed_profile(),
            });

        let restart_seed_ids = restart_seeds
            .iter()
            .map(|(restart_id, _)| restart_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            restart_seed_ids,
            vec![
                "return_clock_buy_excitation_up",
                "return_clock_buy_excitation_down",
                "return_clock_sell_excitation_up",
                "return_clock_sell_excitation_down",
            ]
        );

        let buy_mesh_radius = initial_restart_mesh_radius_for_coordinate(
            active_parameter_layout.log_buy_excitation_return_clock_scale_index,
            baseline_parameter_vector.as_slice(),
            lower_bounds.as_slice(),
            upper_bounds.as_slice(),
        )
        .expect("test bounds have positive width");

        let buy_up_seed = restart_seeds
            .iter()
            .find(|(restart_id, _)| restart_id == "return_clock_buy_excitation_up")
            .map(|(_, restart_seed)| restart_seed.as_slice())
            .expect("buy-up return-clock seed should be present");

        let expected_log_delta_shift = stationary_return_clock_log_delta_compensation(
            test_return_clock_seed_profile(),
            0.0,
            0.0,
            buy_mesh_radius,
            0.0,
        )
        .expect("finite stationary compensation should be available");

        assert_eq!(
            buy_up_seed[active_parameter_layout.log_buy_excitation_return_clock_scale_index],
            buy_mesh_radius
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
}
