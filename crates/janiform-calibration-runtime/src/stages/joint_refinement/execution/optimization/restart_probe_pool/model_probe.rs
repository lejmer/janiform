use janiform_foundations::linear_algebra::scale_vector_to_euclidean_radius;

use super::super::super::JointRefinementActiveParameterLayout;
use super::super::parameter_groups::ParameterGroupProbeDirection;
use super::super::parameter_groups::parameter_group_probe_directions;
use super::super::parameter_space::{
    denormalize_parameter_vector, normalized_parameter_vector, parameter_step_is_zero,
};
use super::trust_region::TrustRegionCandidate;
use super::{RestartProbePrediction, RestartProbeState};

pub(super) fn trust_region_model_probe_vectors(
    state: &RestartProbeState,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
) -> Vec<TrustRegionCandidate> {
    let Some(normalized_accepted) =
        normalized_parameter_vector(state.accepted_vector.as_slice(), lower_bounds, upper_bounds)
    else {
        return Vec::new();
    };

    let directions = parameter_group_probe_directions(
        "model_probe",
        state.accepted_vector.as_slice(),
        lower_bounds,
        upper_bounds,
        active_parameter_layout,
    );

    let mut candidates = Vec::with_capacity(directions.len());

    for direction in directions {
        push_model_probe_candidate(
            &mut candidates,
            direction,
            normalized_accepted.as_slice(),
            state.accepted_vector.as_slice(),
            lower_bounds,
            upper_bounds,
            state.trust_region_radius,
        );
    }

    candidates
}

fn push_model_probe_candidate(
    candidates: &mut Vec<TrustRegionCandidate>,
    direction: ParameterGroupProbeDirection,
    normalized_accepted: &[f64],
    accepted_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    trust_region_radius: f64,
) {
    let Some(trusted_step) =
        scale_vector_to_euclidean_radius(direction.direction.as_slice(), trust_region_radius)
    else {
        return;
    };

    if parameter_step_is_zero(trusted_step.as_slice()) {
        return;
    }

    let normalized_candidate = normalized_accepted
        .iter()
        .zip(trusted_step.iter())
        .map(|(coordinate, step)| (*coordinate + *step).clamp(0.0, 1.0))
        .collect::<Vec<_>>();

    let Some(parameter_vector) =
        denormalize_parameter_vector(normalized_candidate.as_slice(), lower_bounds, upper_bounds)
    else {
        return;
    };

    if parameter_vector == accepted_vector
        || candidates
            .iter()
            .any(|candidate| candidate.parameter_vector == parameter_vector)
    {
        return;
    }

    candidates.push(TrustRegionCandidate {
        parameter_vector,
        label: direction.label,
        prediction: RestartProbePrediction::default(),
    });
}

#[cfg(test)]
mod tests {
    use crate::stages::joint_refinement::execution::JointRefinementActiveParameterLayout;

    use super::super::restart_probe_state_for_test;
    use super::trust_region_model_probe_vectors;

    fn test_active_parameter_layout() -> JointRefinementActiveParameterLayout {
        JointRefinementActiveParameterLayout {
            hawkes_block: vec![0, 1, 2, 3],
            nig_block: vec![4, 5, 6, 7],
            return_clock_block: vec![8, 9],
            long_variance_block: vec![10, 11, 12, 13, 14],
            option_pricing_kernel_block: vec![15, 16, 17],
            coupling_block: vec![18],
            log_stationary_event_rate_scale_index: 0,
            log_beta_scale_index: 1,
            log_self_excitation_scale_index: 2,
            log_cross_excitation_scale_index: 3,
            log_buy_excitation_return_clock_scale_index: 8,
            log_sell_excitation_return_clock_scale_index: 9,
            log_long_variance_mean_reversion_rate_scale_index: 10,
            long_variance_vol_of_vol_index: 11,
            long_variance_return_energy_loading_index: 12,
            long_variance_buy_event_loading_index: 13,
            long_variance_sell_event_loading_index: 14,
            option_pricing_log_buy_activity_premium_index: 15,
            option_pricing_log_sell_activity_premium_index: 16,
            option_pricing_pressure_signed_imbalance_exposure_risk_price_index: 17,
            known_activity_scale_index: None,
            surprise_activity_scale_index: None,
            surprise_baseline_scale_index: None,
        }
    }

    #[test]
    fn model_probe_vectors_use_mechanism_groups() {
        let state = restart_probe_state_for_test(vec![0.5; 19], 0.25);

        let lower_bounds = vec![0.0; 19];
        let upper_bounds = vec![1.0; 19];

        let candidates = trust_region_model_probe_vectors(
            &state,
            lower_bounds.as_slice(),
            upper_bounds.as_slice(),
            &test_active_parameter_layout(),
        );

        let labels = candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>();

        assert!(labels.contains(&"model_probe.nig_tail_scale.positive"));
        assert!(labels.contains(&"model_probe.nig_skew_balance.negative"));
        assert!(labels.contains(&"model_probe.all_mechanisms.positive"));
    }
}
