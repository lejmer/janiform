use super::{RestartProbeState, restart_probe_state_has_remaining_budget};

pub(super) fn racing_restart_positions(
    states: &[RestartProbeState],
    round_robin_start_position: usize,
    max_in_flight_per_restart: usize,
) -> Vec<usize> {
    let initial_positions = initial_evidence_positions(
        states,
        round_robin_start_position,
        max_in_flight_per_restart,
    );
    if !initial_positions.is_empty() {
        return initial_positions;
    }

    racing_positions_after_initial_evidence(
        states,
        round_robin_start_position,
        max_in_flight_per_restart,
    )
}

fn initial_evidence_positions(
    states: &[RestartProbeState],
    round_robin_start_position: usize,
    max_in_flight_per_restart: usize,
) -> Vec<usize> {
    round_robin_positions(states.len(), round_robin_start_position)
        .into_iter()
        .filter(|state_position| {
            let state = &states[*state_position];
            restart_state_can_receive_search_work(state, max_in_flight_per_restart)
                && state.evaluation_count == 0
                && state.in_flight_count == 0
        })
        .collect()
}

fn racing_positions_after_initial_evidence(
    states: &[RestartProbeState],
    round_robin_start_position: usize,
    max_in_flight_per_restart: usize,
) -> Vec<usize> {
    let racing_weights = racing_weights_by_state_position(states);

    let total_weight = racing_weights
        .iter()
        .filter_map(|weight| *weight)
        .sum::<f64>();

    let scheduled_after_initial = states
        .iter()
        .map(|state| {
            state
                .evaluation_count
                .saturating_add(state.in_flight_count)
                .saturating_sub(usize::from(state.initial_objective.is_some()))
        })
        .sum::<usize>();

    let mut candidates = round_robin_positions(states.len(), round_robin_start_position)
        .into_iter()
        .filter_map(|state_position| {
            let state = &states[state_position];
            if !restart_state_can_receive_search_work(state, max_in_flight_per_restart) {
                return None;
            }

            let best_objective = restart_state_best_objective(state)?;
            let racing_weight = racing_weights.get(state_position).copied().flatten()?;
            let scheduled_state_after_initial = state
                .evaluation_count
                .saturating_add(state.in_flight_count)
                .saturating_sub(usize::from(state.initial_objective.is_some()));

            let budget_debt = if total_weight > 0.0 && racing_weight > 0.0 {
                (scheduled_after_initial as f64 * racing_weight / total_weight)
                    - scheduled_state_after_initial as f64
            } else {
                -(scheduled_state_after_initial as f64)
            };

            Some(RacingCandidate {
                state_position,
                budget_debt,
                best_objective,
                round_robin_distance: round_robin_distance(
                    states.len(),
                    round_robin_start_position,
                    state_position,
                ),
            })
        })
        .collect::<Vec<_>>();

    candidates.sort_by(compare_racing_candidates);

    candidates
        .into_iter()
        .map(|candidate| candidate.state_position)
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct RacingCandidate {
    state_position: usize,
    budget_debt: f64,
    best_objective: f64,
    round_robin_distance: usize,
}

fn compare_racing_candidates(
    left_candidate: &RacingCandidate,
    right_candidate: &RacingCandidate,
) -> std::cmp::Ordering {
    right_candidate
        .budget_debt
        .total_cmp(&left_candidate.budget_debt)
        .then_with(|| {
            left_candidate
                .best_objective
                .total_cmp(&right_candidate.best_objective)
        })
        .then_with(|| {
            left_candidate
                .round_robin_distance
                .cmp(&right_candidate.round_robin_distance)
        })
}

fn racing_weights_by_state_position(states: &[RestartProbeState]) -> Vec<Option<f64>> {
    let finite_best_objectives = states
        .iter()
        .filter_map(restart_state_best_objective)
        .collect::<Vec<_>>();

    if finite_best_objectives.is_empty() {
        return vec![None; states.len()];
    }

    let best_objective = finite_best_objectives
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let worst_objective = finite_best_objectives
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let objective_range = worst_objective - best_objective;

    let finite_improvements = states
        .iter()
        .filter_map(restart_state_nonnegative_improvement)
        .collect::<Vec<_>>();
    let maximum_improvement = finite_improvements.iter().copied().fold(0.0_f64, f64::max);

    states
        .iter()
        .map(|state| {
            let best_loss_score = restart_state_best_objective(state).map(|objective| {
                if objective_range > 0.0 {
                    (worst_objective - objective) / objective_range
                } else {
                    1.0
                }
            })?;

            let improvement_score =
                restart_state_nonnegative_improvement(state).map(|improvement| {
                    if maximum_improvement > 0.0 {
                        improvement / maximum_improvement
                    } else {
                        0.0
                    }
                })?;

            Some(best_loss_score.max(improvement_score))
        })
        .collect()
}

fn restart_state_can_receive_search_work(
    state: &RestartProbeState,
    max_in_flight_per_restart: usize,
) -> bool {
    !state.completed
        && restart_probe_state_has_remaining_budget(state)
        && state.in_flight_count < max_in_flight_per_restart.max(1)
}

fn restart_state_best_objective(state: &RestartProbeState) -> Option<f64> {
    [
        state.best_objective,
        state.round_best_objective,
        state.accepted_objective,
    ]
    .into_iter()
    .flatten()
    .filter(|objective| objective.is_finite())
    .min_by(|left, right| left.total_cmp(right))
}

fn restart_state_nonnegative_improvement(state: &RestartProbeState) -> Option<f64> {
    let initial_objective = state.initial_objective?;
    let best_objective = restart_state_best_objective(state)?;

    (initial_objective.is_finite() && best_objective.is_finite())
        .then_some((initial_objective - best_objective).max(0.0))
}

fn round_robin_positions(state_count: usize, start_position: usize) -> Vec<usize> {
    (0..state_count)
        .map(|offset| (start_position + offset) % state_count)
        .collect()
}

fn round_robin_distance(state_count: usize, start_position: usize, state_position: usize) -> usize {
    if state_count == 0 {
        return 0;
    }

    (state_position + state_count - (start_position % state_count)) % state_count
}

#[cfg(test)]
mod tests {
    use super::super::RestartProbeState;
    use super::racing_restart_positions;

    fn restart_state(restart_id: &str, initial_objective: Option<f64>) -> RestartProbeState {
        RestartProbeState {
            restart_id: restart_id.to_string(),
            seed_vector: vec![0.0],
            accepted_vector: vec![0.0],
            best_vector: vec![0.0],
            initial_objective,
            accepted_objective: initial_objective,
            best_objective: initial_objective,
            round_best_vector: None,
            round_best_objective: None,
            round_best_residuals: None,
            round_best_predicted_residual_objective: None,
            accepted_residuals: None,
            evaluated_records: Vec::new(),
            scheduled_parameter_bits: Vec::new(),
            model_probe_vectors: Vec::new(),
            next_model_probe_position: 0,
            model_probes_prepared: false,
            trust_region_proposal_vectors: Vec::new(),
            next_trust_region_proposal_position: 0,
            trust_region_proposals_prepared: false,
            trust_region_radius: 0.25,
            completed_search_rounds: 0,
            evaluation_count: usize::from(initial_objective.is_some()),
            max_evaluation_count: 8,
            in_flight_count: 0,
            stop_reason: "running".to_string(),
            completed: false,
            exact_likelihood_evaluator_counts: None,
        }
    }

    #[test]
    fn racing_schedules_missing_initial_evidence_first() {
        let states = vec![
            restart_state("evaluated", Some(5.0)),
            restart_state("pending", None),
        ];

        assert_eq!(racing_restart_positions(states.as_slice(), 0, 1), vec![1]);
    }

    #[test]
    fn racing_prioritizes_low_loss_restart_after_initial_evidence() {
        let states = vec![
            restart_state("weak", Some(10.0)),
            restart_state("strong", Some(2.0)),
            restart_state("middle", Some(5.0)),
        ];

        assert_eq!(
            racing_restart_positions(states.as_slice(), 0, 1),
            vec![1, 2, 0]
        );
    }

    #[test]
    fn racing_accounts_for_early_improvement() {
        let mut improving = restart_state("improving", Some(10.0));
        improving.best_objective = Some(4.0);
        improving.evaluation_count = 4;

        let mut lower_initial = restart_state("lower_initial", Some(5.0));
        lower_initial.evaluation_count = 4;

        let states = vec![lower_initial, improving];

        assert_eq!(
            racing_restart_positions(states.as_slice(), 0, 1),
            vec![1, 0]
        );
    }
}
