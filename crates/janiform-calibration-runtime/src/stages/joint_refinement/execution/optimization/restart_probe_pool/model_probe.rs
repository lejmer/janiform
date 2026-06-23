use janiform_foundations::linear_algebra::scale_vector_to_euclidean_radius;

use super::super::super::JointRefinementActiveParameterLayout;
use super::super::parameter_space::{
    denormalize_parameter_vector, movable_parameter_positions, normalized_parameter_vector,
    parameter_step_is_zero,
};
use super::trust_region::TrustRegionCandidate;
use super::{RestartProbePrediction, RestartProbeState};

#[derive(Debug, Clone)]
struct TrustRegionModelProbeDirection {
    direction: Vec<f64>,
    label: String,
}

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

    let directions = trust_region_model_probe_directions(
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

fn trust_region_model_probe_directions(
    accepted_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
) -> Vec<TrustRegionModelProbeDirection> {
    let movable_positions =
        movable_parameter_positions(accepted_vector, lower_bounds, upper_bounds);
    if movable_positions.is_empty() {
        return Vec::new();
    }

    let mut directions = Vec::new();

    for group in mechanism_probe_groups(active_parameter_layout) {
        push_group_probe_directions(
            &mut directions,
            group.label,
            group.parameter_positions.as_slice(),
            accepted_vector.len(),
            movable_positions.as_slice(),
        );
    }

    if directions.is_empty() {
        push_group_probe_directions(
            &mut directions,
            "all_movable",
            movable_positions.as_slice(),
            accepted_vector.len(),
            movable_positions.as_slice(),
        );
    }

    directions
}

#[derive(Debug, Clone)]
struct MechanismProbeGroup {
    label: &'static str,
    parameter_positions: Vec<usize>,
}

fn mechanism_probe_groups(
    active_parameter_layout: &JointRefinementActiveParameterLayout,
) -> Vec<MechanismProbeGroup> {
    let mut groups = Vec::new();

    push_mechanism_probe_group(
        &mut groups,
        "hawkes",
        active_parameter_layout.hawkes_block.as_slice(),
    );

    if let (Some(log_alpha), Some(log_delta_offset)) = (
        active_parameter_layout.nig_block.first(),
        active_parameter_layout.nig_block.get(3),
    ) {
        let parameter_positions = [*log_alpha, *log_delta_offset];

        push_mechanism_probe_group(
            &mut groups,
            "nig_tail_scale",
            parameter_positions.as_slice(),
        );
    }

    if let (Some(buy_skew), Some(sell_relative)) = (
        active_parameter_layout.nig_block.get(1),
        active_parameter_layout.nig_block.get(2),
    ) {
        let parameter_positions = [*buy_skew, *sell_relative];

        push_mechanism_probe_group(
            &mut groups,
            "nig_skew_balance",
            parameter_positions.as_slice(),
        );
    }

    push_mechanism_probe_group(
        &mut groups,
        "return_clock",
        active_parameter_layout.return_clock_block.as_slice(),
    );

    let long_variance_level_positions = [
        active_parameter_layout.log_long_variance_mean_reversion_rate_scale_index,
        active_parameter_layout.long_variance_vol_of_vol_index,
        active_parameter_layout.long_variance_return_energy_loading_index,
    ];

    push_mechanism_probe_group(
        &mut groups,
        "long_variance_level",
        long_variance_level_positions.as_slice(),
    );

    let long_variance_event_positions = [
        active_parameter_layout.long_variance_buy_event_loading_index,
        active_parameter_layout.long_variance_sell_event_loading_index,
    ];

    push_mechanism_probe_group(
        &mut groups,
        "long_variance_event_loading",
        long_variance_event_positions.as_slice(),
    );

    push_mechanism_probe_group(
        &mut groups,
        "coupling",
        active_parameter_layout.coupling_block.as_slice(),
    );

    push_mechanism_probe_group(
        &mut groups,
        "option_pricing_kernel",
        active_parameter_layout
            .option_pricing_kernel_block
            .as_slice(),
    );

    if let Some(parameter_position) = active_parameter_layout.known_activity_scale_index {
        push_mechanism_probe_group(&mut groups, "known_news_activity", &[parameter_position]);
    }

    if let Some(parameter_position) = active_parameter_layout.surprise_activity_scale_index {
        push_mechanism_probe_group(&mut groups, "surprise_activity", &[parameter_position]);
    }

    if let Some(parameter_position) = active_parameter_layout.surprise_baseline_scale_index {
        push_mechanism_probe_group(&mut groups, "surprise_baseline", &[parameter_position]);
    }

    let all_parameters = active_parameter_layout
        .hawkes_block
        .iter()
        .chain(active_parameter_layout.nig_block.iter())
        .chain(active_parameter_layout.return_clock_block.iter())
        .chain(active_parameter_layout.long_variance_block.iter())
        .chain(active_parameter_layout.coupling_block.iter())
        .chain(active_parameter_layout.option_pricing_kernel_block.iter())
        .copied()
        .collect::<Vec<_>>();

    push_mechanism_probe_group(&mut groups, "all_mechanisms", all_parameters.as_slice());

    groups
}

fn push_mechanism_probe_group(
    groups: &mut Vec<MechanismProbeGroup>,
    label: &'static str,
    parameter_positions: &[usize],
) {
    if parameter_positions.is_empty() {
        return;
    }

    groups.push(MechanismProbeGroup {
        label,
        parameter_positions: parameter_positions.to_vec(),
    });
}

fn push_group_probe_directions(
    directions: &mut Vec<TrustRegionModelProbeDirection>,
    group_label: &str,
    parameter_positions: &[usize],
    parameter_count: usize,
    movable_positions: &[usize],
) {
    let active_positions = parameter_positions
        .iter()
        .copied()
        .filter(|parameter_position| movable_positions.contains(parameter_position))
        .collect::<Vec<_>>();
    if active_positions.is_empty() {
        return;
    }

    let normalized_component = (active_positions.len() as f64).sqrt().recip();

    for (sign_label, sign) in [("positive", 1.0), ("negative", -1.0)] {
        let mut direction = vec![0.0; parameter_count];

        for parameter_position in &active_positions {
            direction[*parameter_position] = sign * normalized_component;
        }

        directions.push(TrustRegionModelProbeDirection {
            direction,
            label: format!("model_probe.{group_label}.{sign_label}"),
        });
    }
}

fn push_model_probe_candidate(
    candidates: &mut Vec<TrustRegionCandidate>,
    direction: TrustRegionModelProbeDirection,
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
