use std::collections::BTreeMap;
use std::sync::mpsc::Sender;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageCandidateDiagnostic, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate,
};

use super::super::objective::{
    JointCandidateBreakdownComparisonInput, compare_joint_candidate_breakdowns,
    joint_primary_metrics_from_breakdown,
};
use super::coarse_consistency::{
    JointRefinementCoarseConsistencyArtifacts, JointRefinementCoarseConsistencyProgress,
    build_joint_candidate_coarse_consistency_artifacts,
};
use super::{
    JointRefinementActiveParameterLayout, JointRefinementCandidateOutcome, JointRefinementContext,
    ordered_parameter_bits,
};

#[derive(Debug, Clone)]
struct JointRefinementCandidateOutcomeGroup<'a> {
    parameter_bits: Vec<u64>,
    representative: &'a JointRefinementCandidateOutcome,
    candidate_outcomes: Vec<&'a JointRefinementCandidateOutcome>,
}

#[derive(Debug, Clone, Copy)]
struct JointRefinementCandidateOrderingState<'a> {
    pareto_rank: usize,
    candidate_outcome: &'a JointRefinementCandidateOutcome,
    admissible: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementLazyCoarseConsistencySelection<'a> {
    pub(crate) selected_candidate_outcomes: Vec<&'a JointRefinementCandidateOutcome>,
    pub(crate) coarse_consistency_artifacts_by_candidate_id:
        BTreeMap<String, JointRefinementCoarseConsistencyArtifacts>,
}

#[derive(Debug, Default, Clone)]
struct LazyCoarseConsistencyCache {
    artifacts_by_candidate_id: BTreeMap<String, JointRefinementCoarseConsistencyArtifacts>,
    artifacts_by_parameter_bits: BTreeMap<Vec<u64>, JointRefinementCoarseConsistencyArtifacts>,
}

#[derive(Debug, Default, Clone)]
struct LazyCoarseConsistencyProgressState {
    completed_units: usize,
    best_candidate_id: Option<String>,
    best_global_loss: Option<f64>,
    best_coarse_consistency_fit: Option<f64>,
}

impl LazyCoarseConsistencyProgressState {
    fn record(
        &mut self,
        candidate_id: &str,
        current_global_loss: f64,
        current_artifacts: Option<&JointRefinementCoarseConsistencyArtifacts>,
    ) -> Option<f64> {
        self.completed_units += 1;

        match self.best_global_loss {
            Some(current_best) if current_global_loss >= current_best => {}
            _ => {
                self.best_global_loss = Some(current_global_loss);
                self.best_candidate_id = Some(candidate_id.to_string());
            }
        }

        let current_coarse_consistency_fit = current_artifacts
            .as_ref()
            .map(|artifacts| artifacts.summary.raw_fit);

        if let Some(artifacts) = current_artifacts {
            self.best_coarse_consistency_fit = Some(match self.best_coarse_consistency_fit {
                Some(current_best) => current_best.min(artifacts.summary.raw_fit),
                None => artifacts.summary.raw_fit,
            });
        }

        current_coarse_consistency_fit
    }
}

pub(crate) fn admissibility_threshold_from_sample_count(sample_count: usize) -> f64 {
    1.0 + (sample_count.max(1) as f64).recip().sqrt()
}

pub(crate) fn select_candidate_outcomes_with_lazy_coarse_consistency<'a>(
    candidate_outcomes: &'a [&'a JointRefinementCandidateOutcome],
    context: &JointRefinementContext,
    baseline_artifacts: Option<&JointRefinementCoarseConsistencyArtifacts>,
    selected_candidate_count: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> JointRefinementLazyCoarseConsistencySelection<'a> {
    let selected_candidate_count = selected_candidate_count.max(1);

    let candidate_groups = build_joint_candidate_outcome_groups(candidate_outcomes);

    let representative_candidate_outcomes =
        representative_candidate_outcomes(candidate_groups.as_slice());

    let optimistic_candidate_states =
        build_joint_candidate_ordering_states(representative_candidate_outcomes.as_slice());

    let has_optimistically_admissible_candidate = optimistic_candidate_states
        .iter()
        .any(|candidate_state| candidate_state.admissible);

    let mut ranked_candidate_seeds = optimistic_candidate_states.clone();

    ranked_candidate_seeds.sort_by(|left, right| {
        compare_joint_candidate_ordering_states(
            *left,
            *right,
            has_optimistically_admissible_candidate,
        )
    });

    let Some(baseline_artifacts) = baseline_artifacts else {
        return JointRefinementLazyCoarseConsistencySelection {
            selected_candidate_outcomes:
                select_ranked_candidate_outcomes_without_coarse_consistency(
                    candidate_outcomes,
                    candidate_groups.as_slice(),
                    ranked_candidate_seeds.as_slice(),
                    selected_candidate_count,
                ),
            coarse_consistency_artifacts_by_candidate_id: BTreeMap::new(),
        };
    };

    let total_ranked_candidate_groups = ranked_candidate_seeds.len();

    let mut coarse_consistency_cache = LazyCoarseConsistencyCache::default();

    seed_baseline_coarse_consistency(
        &mut coarse_consistency_cache,
        candidate_groups.as_slice(),
        baseline_artifacts,
    );

    emit_coarse_consistency_progress(CoarseConsistencyProgressEmissionInput {
        progress_sender,
        completed_units: 0,
        total_units: total_ranked_candidate_groups,
        current_item_label: None,
        current_global_loss: None,
        best_candidate_id: None,
        best_global_loss: None,
        current_coarse_consistency_fit: None,
        best_coarse_consistency_fit: None,
    });

    let mut progress_state = LazyCoarseConsistencyProgressState::default();
    let mut admissible_candidate_outcomes = Vec::new();
    let mut fallback_candidate_outcomes = Vec::new();

    for candidate_state in ranked_candidate_seeds {
        let Some(candidate_group) = find_candidate_group(
            candidate_groups.as_slice(),
            candidate_state.candidate_outcome.candidate_id.as_str(),
        ) else {
            continue;
        };

        let current_global_loss = candidate_state
            .candidate_outcome
            .final_evaluation
            .breakdown
            .global_loss;

        emit_coarse_consistency_progress(CoarseConsistencyProgressEmissionInput {
            progress_sender,
            completed_units: progress_state.completed_units,
            total_units: total_ranked_candidate_groups,
            current_item_label: Some(candidate_state.candidate_outcome.candidate_id.as_str()),
            current_global_loss: Some(current_global_loss),
            best_candidate_id: progress_state.best_candidate_id.as_deref(),
            best_global_loss: progress_state.best_global_loss,
            current_coarse_consistency_fit: None,
            best_coarse_consistency_fit: progress_state.best_coarse_consistency_fit,
        });

        let current_artifacts = if candidate_state.admissible {
            resolve_candidate_group_coarse_consistency(
                &mut coarse_consistency_cache,
                context,
                candidate_group,
                progress_sender,
            )
        } else {
            None
        };

        let current_coarse_consistency_fit = progress_state.record(
            candidate_state.candidate_outcome.candidate_id.as_str(),
            current_global_loss,
            current_artifacts.as_ref(),
        );

        emit_coarse_consistency_progress(CoarseConsistencyProgressEmissionInput {
            progress_sender,
            completed_units: progress_state.completed_units,
            total_units: total_ranked_candidate_groups,
            current_item_label: Some(candidate_state.candidate_outcome.candidate_id.as_str()),
            current_global_loss: Some(current_global_loss),
            best_candidate_id: progress_state.best_candidate_id.as_deref(),
            best_global_loss: progress_state.best_global_loss,
            current_coarse_consistency_fit,
            best_coarse_consistency_fit: progress_state.best_coarse_consistency_fit,
        });

        if candidate_state.admissible
            && let Some(selected_candidate_outcome) = find_candidate_outcome(
                candidate_outcomes,
                candidate_group.representative.candidate_id.as_str(),
            )
        {
            admissible_candidate_outcomes.push(selected_candidate_outcome);

            if admissible_candidate_outcomes.len() >= selected_candidate_count {
                return JointRefinementLazyCoarseConsistencySelection {
                    selected_candidate_outcomes: admissible_candidate_outcomes,
                    coarse_consistency_artifacts_by_candidate_id: coarse_consistency_cache
                        .artifacts_by_candidate_id,
                };
            }
        } else if let Some(fallback_candidate_outcome) = find_candidate_outcome(
            candidate_outcomes,
            candidate_group.representative.candidate_id.as_str(),
        ) {
            fallback_candidate_outcomes.push(fallback_candidate_outcome);
        }
    }

    let selected_candidate_outcomes = selected_candidate_outcomes_from_ranked_buckets(
        admissible_candidate_outcomes,
        fallback_candidate_outcomes,
        selected_candidate_count,
    );

    JointRefinementLazyCoarseConsistencySelection {
        selected_candidate_outcomes,
        coarse_consistency_artifacts_by_candidate_id: coarse_consistency_cache
            .artifacts_by_candidate_id,
    }
}

fn select_ranked_candidate_outcomes_without_coarse_consistency<'a>(
    candidate_outcomes: &[&'a JointRefinementCandidateOutcome],
    candidate_groups: &[JointRefinementCandidateOutcomeGroup<'a>],
    ranked_candidate_seeds: &[JointRefinementCandidateOrderingState<'a>],
    selected_candidate_count: usize,
) -> Vec<&'a JointRefinementCandidateOutcome> {
    let mut admissible_candidate_outcomes = Vec::new();
    let mut fallback_candidate_outcomes = Vec::new();

    for candidate_state in ranked_candidate_seeds {
        let Some(candidate_group) = find_candidate_group(
            candidate_groups,
            candidate_state.candidate_outcome.candidate_id.as_str(),
        ) else {
            continue;
        };

        if candidate_state.admissible
            && let Some(selected_candidate_outcome) = find_candidate_outcome(
                candidate_outcomes,
                candidate_group.representative.candidate_id.as_str(),
            )
        {
            admissible_candidate_outcomes.push(selected_candidate_outcome);
        } else if let Some(fallback_candidate_outcome) = find_candidate_outcome(
            candidate_outcomes,
            candidate_group.representative.candidate_id.as_str(),
        ) {
            fallback_candidate_outcomes.push(fallback_candidate_outcome);
        }
    }

    selected_candidate_outcomes_from_ranked_buckets(
        admissible_candidate_outcomes,
        fallback_candidate_outcomes,
        selected_candidate_count,
    )
}

fn selected_candidate_outcomes_from_ranked_buckets<'a>(
    mut selected_candidate_outcomes: Vec<&'a JointRefinementCandidateOutcome>,
    fallback_candidate_outcomes: Vec<&'a JointRefinementCandidateOutcome>,
    selected_candidate_count: usize,
) -> Vec<&'a JointRefinementCandidateOutcome> {
    for fallback_candidate_outcome in fallback_candidate_outcomes {
        if selected_candidate_outcomes.len() >= selected_candidate_count {
            break;
        }

        if selected_candidate_outcomes
            .iter()
            .any(|selected_candidate_outcome| {
                selected_candidate_outcome.candidate_id == fallback_candidate_outcome.candidate_id
            })
        {
            continue;
        }

        selected_candidate_outcomes.push(fallback_candidate_outcome);
    }

    selected_candidate_outcomes
}

fn representative_candidate_outcomes<'a>(
    candidate_groups: &[JointRefinementCandidateOutcomeGroup<'a>],
) -> Vec<&'a JointRefinementCandidateOutcome> {
    candidate_groups
        .iter()
        .map(|candidate_group| candidate_group.representative)
        .collect()
}

fn find_candidate_group<'a>(
    candidate_groups: &'a [JointRefinementCandidateOutcomeGroup<'a>],
    candidate_id: &str,
) -> Option<&'a JointRefinementCandidateOutcomeGroup<'a>> {
    candidate_groups
        .iter()
        .find(|candidate_group| candidate_group.representative.candidate_id == candidate_id)
}

fn find_candidate_outcome<'a>(
    candidate_outcomes: &[&'a JointRefinementCandidateOutcome],
    candidate_id: &str,
) -> Option<&'a JointRefinementCandidateOutcome> {
    candidate_outcomes
        .iter()
        .copied()
        .find(|candidate_outcome| candidate_outcome.candidate_id == candidate_id)
}

fn seed_baseline_coarse_consistency(
    coarse_consistency_cache: &mut LazyCoarseConsistencyCache,
    candidate_groups: &[JointRefinementCandidateOutcomeGroup<'_>],
    baseline_artifacts: &JointRefinementCoarseConsistencyArtifacts,
) {
    for candidate_group in candidate_groups {
        if candidate_group
            .candidate_outcomes
            .iter()
            .any(|candidate_outcome| candidate_outcome.candidate_id == "baseline")
        {
            coarse_consistency_cache.artifacts_by_parameter_bits.insert(
                candidate_group.parameter_bits.clone(),
                baseline_artifacts.clone(),
            );

            assign_coarse_consistency_artifacts_to_candidate_group(
                &mut coarse_consistency_cache.artifacts_by_candidate_id,
                candidate_group,
                baseline_artifacts,
            );
        }
    }
}

fn resolve_candidate_group_coarse_consistency(
    coarse_consistency_cache: &mut LazyCoarseConsistencyCache,
    context: &JointRefinementContext,
    candidate_group: &JointRefinementCandidateOutcomeGroup<'_>,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Option<JointRefinementCoarseConsistencyArtifacts> {
    if let Some(artifacts) = coarse_consistency_cache
        .artifacts_by_parameter_bits
        .get(&candidate_group.parameter_bits)
        .cloned()
    {
        assign_coarse_consistency_artifacts_to_candidate_group(
            &mut coarse_consistency_cache.artifacts_by_candidate_id,
            candidate_group,
            &artifacts,
        );

        return Some(artifacts);
    }

    let resolved_artifacts = build_joint_candidate_coarse_consistency_artifacts(
        context,
        &candidate_group
            .representative
            .final_evaluation
            .fitted_parameters,
        Some(candidate_group.parameter_bits.as_slice()),
        Some(JointRefinementCoarseConsistencyProgress {
            sender: progress_sender,
            phase: JointRefinementStageProgressPhase::CoarseConsistencyEvaluation,
            current_item_label: Some(candidate_group.representative.candidate_id.as_str()),
        }),
    )?;

    coarse_consistency_cache.artifacts_by_parameter_bits.insert(
        candidate_group.parameter_bits.clone(),
        resolved_artifacts.clone(),
    );

    assign_coarse_consistency_artifacts_to_candidate_group(
        &mut coarse_consistency_cache.artifacts_by_candidate_id,
        candidate_group,
        &resolved_artifacts,
    );

    Some(resolved_artifacts)
}

pub(crate) fn build_joint_candidate_frontier_diagnostics(
    candidate_outcomes: &[JointRefinementCandidateOutcome],
    context: &JointRefinementContext,
) -> Vec<JointRefinementStageCandidateDiagnostic> {
    if candidate_outcomes.is_empty() {
        return Vec::new();
    }

    let candidate_outcome_refs = candidate_outcomes.iter().collect::<Vec<_>>();

    let candidate_states = build_joint_candidate_ordering_states(candidate_outcome_refs.as_slice());

    let has_admissible_candidate = candidate_states
        .iter()
        .any(|candidate_state| candidate_state.admissible);

    let mut ranked_candidates = candidate_states
        .iter()
        .map(|candidate_state| (*candidate_state, candidate_state.candidate_outcome))
        .collect::<Vec<_>>();

    ranked_candidates.sort_by(|left, right| {
        compare_joint_candidate_ordering_states(left.0, right.0, has_admissible_candidate)
    });

    let lexicographic_rank_by_candidate_id: BTreeMap<&str, usize> = ranked_candidates
        .iter()
        .enumerate()
        .map(|(position, (_, outcome))| (outcome.candidate_id.as_str(), position + 1))
        .collect();

    candidate_outcomes
        .iter()
        .map(|outcome| {
            let candidate_state = candidate_states
                .iter()
                .find(|candidate_state| {
                    candidate_state.candidate_outcome.candidate_id == outcome.candidate_id
                })
                .copied()
                .unwrap_or(JointRefinementCandidateOrderingState {
                    pareto_rank: usize::MAX,
                    candidate_outcome: outcome,
                    admissible: false,
                });

            let valid = outcome.local_evaluation.invalid_reason.is_none()
                && outcome.final_evaluation.invalid_reason.is_none();

            let invalid_reason = outcome
                .final_evaluation
                .invalid_reason
                .clone()
                .or_else(|| outcome.local_evaluation.invalid_reason.clone());

            JointRefinementStageCandidateDiagnostic {
                candidate_id: format!("joint_refinement_{}", outcome.candidate_id),
                valid,
                admissible: candidate_state.admissible,
                pareto_rank: candidate_state.pareto_rank,
                lexicographic_rank: *lexicographic_rank_by_candidate_id
                    .get(outcome.candidate_id.as_str())
                    .unwrap_or(&(candidate_outcomes.len() + 1)),
                scalar_tie_break: outcome.final_evaluation.breakdown.global_loss,
                local_evaluation_count: outcome.local_evaluation_count,
                stop_reason: outcome.stop_reason.clone(),
                invalid_reason,
                objective_terms: outcome.final_evaluation.objective_terms.clone(),
                objective_breakdown: outcome.final_evaluation.breakdown.clone(),
                canonical_coordinates: named_joint_parameter_coordinates(
                    outcome.parameter_vector.as_slice(),
                    &context.active_parameter_layout,
                ),
            }
        })
        .collect()
}

pub(crate) fn joint_candidate_outcome_is_admissible(
    candidate_outcome: &JointRefinementCandidateOutcome,
) -> bool {
    candidate_outcome
        .final_evaluation
        .breakdown
        .admissibility
        .admissible
}

fn joint_candidate_outcome_dominates(
    left: &JointRefinementCandidateOutcome,
    right: &JointRefinementCandidateOutcome,
) -> bool {
    let left_metrics = joint_primary_metrics_from_breakdown(&left.final_evaluation.breakdown);
    let right_metrics = joint_primary_metrics_from_breakdown(&right.final_evaluation.breakdown);

    let all_not_worse = left_metrics
        .iter()
        .zip(right_metrics.iter())
        .all(|(left_value, right_value)| left_value <= right_value);

    let any_strictly_better = left_metrics
        .iter()
        .zip(right_metrics.iter())
        .any(|(left_value, right_value)| left_value < right_value);

    all_not_worse && any_strictly_better
}

fn build_joint_candidate_outcome_groups<'a>(
    candidate_outcomes: &[&'a JointRefinementCandidateOutcome],
) -> Vec<JointRefinementCandidateOutcomeGroup<'a>> {
    let grouped_candidate_outcomes = candidate_outcomes.iter().copied().fold(
        BTreeMap::<Vec<u64>, Vec<&JointRefinementCandidateOutcome>>::new(),
        |mut grouped_candidate_outcomes, candidate_outcome| {
            grouped_candidate_outcomes
                .entry(ordered_parameter_bits(
                    candidate_outcome.parameter_vector.as_slice(),
                ))
                .or_default()
                .push(candidate_outcome);
            grouped_candidate_outcomes
        },
    );

    grouped_candidate_outcomes
        .into_iter()
        .filter_map(|(parameter_bits, grouped_candidate_outcomes)| {
            let representative = grouped_candidate_outcomes
                .iter()
                .copied()
                .min_by(|left, right| left.candidate_id.cmp(&right.candidate_id))?;
            Some(JointRefinementCandidateOutcomeGroup {
                parameter_bits,
                representative,
                candidate_outcomes: grouped_candidate_outcomes,
            })
        })
        .collect()
}

fn assign_coarse_consistency_artifacts_to_candidate_group(
    coarse_consistency_artifacts_by_candidate_id: &mut BTreeMap<
        String,
        JointRefinementCoarseConsistencyArtifacts,
    >,
    candidate_group: &JointRefinementCandidateOutcomeGroup<'_>,
    artifacts: &JointRefinementCoarseConsistencyArtifacts,
) {
    for candidate_outcome in &candidate_group.candidate_outcomes {
        coarse_consistency_artifacts_by_candidate_id
            .insert(candidate_outcome.candidate_id.clone(), artifacts.clone());
    }
}

fn build_joint_candidate_ordering_states<'a>(
    candidate_outcomes: &[&'a JointRefinementCandidateOutcome],
) -> Vec<JointRefinementCandidateOrderingState<'a>> {
    let pareto_ranks = pareto_ranks(candidate_outcomes);

    candidate_outcomes
        .iter()
        .enumerate()
        .map(
            |(position, outcome)| JointRefinementCandidateOrderingState {
                pareto_rank: pareto_ranks[position],
                candidate_outcome: outcome,
                admissible: outcome.local_evaluation.invalid_reason.is_none()
                    && outcome.final_evaluation.invalid_reason.is_none()
                    && joint_candidate_outcome_is_admissible(outcome),
            },
        )
        .collect()
}

fn pareto_ranks(candidate_outcomes: &[&JointRefinementCandidateOutcome]) -> Vec<usize> {
    candidate_outcomes
        .iter()
        .map(|candidate| {
            1 + candidate_outcomes
                .iter()
                .filter(|other| joint_candidate_outcome_dominates(other, candidate))
                .count()
        })
        .collect()
}

fn compare_joint_candidate_ordering_states(
    left: JointRefinementCandidateOrderingState<'_>,
    right: JointRefinementCandidateOrderingState<'_>,
    has_admissible_candidate: bool,
) -> std::cmp::Ordering {
    compare_joint_candidate_breakdowns(JointCandidateBreakdownComparisonInput {
        left_pareto_rank: left.pareto_rank,
        left_breakdown: &left.candidate_outcome.final_evaluation.breakdown,
        left_admissible: left.admissible,
        left_candidate_id: left.candidate_outcome.candidate_id.as_str(),
        right_pareto_rank: right.pareto_rank,
        right_breakdown: &right.candidate_outcome.final_evaluation.breakdown,
        right_admissible: right.admissible,
        right_candidate_id: right.candidate_outcome.candidate_id.as_str(),
        has_admissible_candidate,
    })
}

fn named_joint_parameter_coordinates(
    parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
) -> Vec<(String, f64)> {
    (0..active_parameter_layout.vector_length())
        .filter_map(|index| {
            let label = joint_parameter_name_by_index(index, active_parameter_layout)?;
            parameter_vector
                .get(index)
                .copied()
                .map(|value| (label, value))
        })
        .collect()
}

pub(crate) fn joint_parameter_name_by_index(
    parameter_index: usize,
    active_parameter_layout: &JointRefinementActiveParameterLayout,
) -> Option<String> {
    let mut parameter_names = vec![
        (
            active_parameter_layout.log_stationary_event_rate_scale_index,
            "log_stationary_event_rate_scale",
        ),
        (
            active_parameter_layout.log_beta_scale_index,
            "log_beta_scale",
        ),
        (
            active_parameter_layout.log_self_excitation_scale_index,
            "log_self_excitation_scale",
        ),
        (
            active_parameter_layout.log_cross_excitation_scale_index,
            "log_cross_excitation_scale",
        ),
        (active_parameter_layout.nig_block[0], "nig.log_alpha"),
        (
            active_parameter_layout.nig_block[1],
            "nig.buy_skew_coordinate",
        ),
        (
            active_parameter_layout.nig_block[2],
            "nig.sell_relative_coordinate",
        ),
        (active_parameter_layout.nig_block[3], "nig.log_delta_offset"),
        (
            active_parameter_layout.log_buy_excitation_return_clock_scale_index,
            "log_buy_excitation_return_clock_scale",
        ),
        (
            active_parameter_layout.log_sell_excitation_return_clock_scale_index,
            "log_sell_excitation_return_clock_scale",
        ),
        (
            active_parameter_layout.log_long_variance_mean_reversion_rate_scale_index,
            "long_variance.log_mean_reversion_rate_scale",
        ),
        (
            active_parameter_layout.long_variance_vol_of_vol_index,
            "long_variance.vol_of_vol",
        ),
        (
            active_parameter_layout.long_variance_return_energy_loading_index,
            "long_variance.return_energy_loading",
        ),
        (
            active_parameter_layout.long_variance_buy_event_loading_index,
            "long_variance.buy_event_loading",
        ),
        (
            active_parameter_layout.long_variance_sell_event_loading_index,
            "long_variance.sell_event_loading",
        ),
    ];

    if let Some(index) = active_parameter_layout.known_activity_scale_index {
        parameter_names.push((index, "known_activity_scale"));
    }

    if let Some(index) = active_parameter_layout.surprise_activity_scale_index {
        parameter_names.push((index, "surprise_activity_scale"));
    }

    if let Some(index) = active_parameter_layout.surprise_baseline_scale_index {
        parameter_names.push((index, "surprise_baseline_scale"));
    }

    parameter_names
        .into_iter()
        .find_map(|(index, label)| (index == parameter_index).then(|| label.to_string()))
}

struct CoarseConsistencyProgressEmissionInput<'a> {
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&'a str>,
    current_global_loss: Option<f64>,
    best_candidate_id: Option<&'a str>,
    best_global_loss: Option<f64>,
    current_coarse_consistency_fit: Option<f64>,
    best_coarse_consistency_fit: Option<f64>,
}

fn emit_coarse_consistency_progress(input: CoarseConsistencyProgressEmissionInput<'_>) {
    let CoarseConsistencyProgressEmissionInput {
        progress_sender,
        completed_units,
        total_units,
        current_item_label,
        current_global_loss,
        best_candidate_id,
        best_global_loss,
        current_coarse_consistency_fit,
        best_coarse_consistency_fit,
    } = input;

    if let Some(progress_sender) = progress_sender {
        let _ = progress_sender.send(JointRefinementStageProgressUpdate {
            current_item_label: current_item_label.map(str::to_string),
            current_global_loss,
            best_candidate_id: best_candidate_id.map(str::to_string),
            best_global_loss,
            current_coarse_consistency_fit,
            best_coarse_consistency_fit,
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::CoarseConsistencyEvaluation,
                completed_units,
                total_units,
            )
        });
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::{
        CalibrationLossAdmissibility, CalibrationLossFamilyDiagnostic,
        JointRefinementStageObjectiveBreakdown,
    };

    use crate::stages::return_components::ContinuousLikelihoodEvaluatorCounts;

    use super::super::test_support::test_joint_fitted_parameters;
    use super::super::{
        JointRefinementCandidateEvaluation, JointRefinementCandidateOutcome,
        JointRefinementRawAdmissibilityMetrics,
    };
    use super::{
        build_joint_candidate_ordering_states, build_joint_candidate_outcome_groups,
        compare_joint_candidate_ordering_states, representative_candidate_outcomes,
        select_ranked_candidate_outcomes_without_coarse_consistency,
    };

    #[test]
    fn missing_baseline_coarse_consistency_selects_valid_ranked_fallback_candidate() {
        let weaker_candidate = build_test_candidate_outcome("weaker_candidate", 3.0);
        let stronger_candidate = build_test_candidate_outcome("stronger_candidate", 1.0);
        let candidate_outcomes = vec![&weaker_candidate, &stronger_candidate];
        let candidate_groups = build_joint_candidate_outcome_groups(candidate_outcomes.as_slice());

        let representative_candidate_outcomes =
            representative_candidate_outcomes(candidate_groups.as_slice());

        let mut ranked_candidate_seeds =
            build_joint_candidate_ordering_states(representative_candidate_outcomes.as_slice());

        let has_admissible_candidate = ranked_candidate_seeds
            .iter()
            .any(|candidate_state| candidate_state.admissible);

        ranked_candidate_seeds.sort_by(|left, right| {
            compare_joint_candidate_ordering_states(*left, *right, has_admissible_candidate)
        });

        let selected_candidate_outcomes =
            select_ranked_candidate_outcomes_without_coarse_consistency(
                candidate_outcomes.as_slice(),
                candidate_groups.as_slice(),
                ranked_candidate_seeds.as_slice(),
                1,
            );

        let selected_candidate_ids = selected_candidate_outcomes
            .iter()
            .map(|candidate_outcome| candidate_outcome.candidate_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(selected_candidate_ids, vec!["stronger_candidate"]);
    }

    fn build_test_candidate_outcome(
        candidate_id: &str,
        global_loss: f64,
    ) -> JointRefinementCandidateOutcome {
        let breakdown = build_test_inadmissible_breakdown(global_loss);

        let evaluation = JointRefinementCandidateEvaluation {
            fitted_parameters: test_joint_fitted_parameters(),
            raw_admissibility_metrics: build_test_raw_admissibility_metrics(global_loss),
            breakdown,
            exact_likelihood_evaluator_counts: ContinuousLikelihoodEvaluatorCounts::default(),
            objective_terms: Vec::new(),
            invalid_reason: None,
        };

        JointRefinementCandidateOutcome {
            candidate_id: candidate_id.to_string(),
            parameter_vector: vec![global_loss],
            local_evaluation: evaluation.clone(),
            final_evaluation: evaluation,
            local_evaluation_count: 1,
            stop_reason: "test".to_string(),
        }
    }

    fn build_test_inadmissible_breakdown(
        global_loss: f64,
    ) -> JointRefinementStageObjectiveBreakdown {
        JointRefinementStageObjectiveBreakdown {
            family_losses: vec![CalibrationLossFamilyDiagnostic {
                family_id: "test_fit".to_string(),
                coordinate_count: 1,
                rank: 1,
                whitened_coordinates: vec![global_loss.sqrt()],
                loss: global_loss,
                max_standardized_residual: global_loss,
                worst_term_id: Some("test_fit".to_string()),
                unidentified: false,
            }],
            term_diagnostics: Vec::new(),
            domain_margin_diagnostics: Vec::new(),
            global_loss,
            admissibility: CalibrationLossAdmissibility {
                admissible: false,
                max_standardized_residual: global_loss,
                excess_loss: global_loss,
                warnings: vec!["test_fit".to_string()],
            },
        }
    }

    fn build_test_raw_admissibility_metrics(value: f64) -> JointRefinementRawAdmissibilityMetrics {
        JointRefinementRawAdmissibilityMetrics {
            aggregated_likelihood_fit: value,
            aggregated_return_shape_fit: value,
            return_asymmetry_fit: value,
            tick_return_asymmetry_fit: value,
            aggregated_return_scale_fit: value,
            tick_return_scale_fit: value,
            business_time_return_scale_fit: value,
            intraday_return_level_fit: value,
            activity_shape_fit: value,
            activity_level_fit: value,
            rv_activity_coupling_fit: value,
            long_variance_horizon_fit: value,
        }
    }
}
