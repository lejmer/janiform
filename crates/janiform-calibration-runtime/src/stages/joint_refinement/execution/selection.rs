use std::collections::BTreeMap;
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::JointRefinementStageObjectiveBreakdown;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageCandidateDiagnostic, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate,
};

use super::super::objective::{
    compare_joint_candidate_breakdowns, joint_primary_metrics_from_breakdown,
};
use super::coarse_consistency::{
    JointRefinementCoarseConsistencyArtifacts, build_joint_candidate_coarse_consistency_artifacts,
};
use super::{
    JointRefinementActiveParameterLayout, JointRefinementCoarseConsistencySummary,
    JointRefinementContext, JointRefinementRestartOutcome, ordered_parameter_bits,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct JointRefinementAdmissibilityThresholds {
    pub(crate) aggregated_likelihood_fit: f64,
    pub(crate) aggregated_return_shape_fit: f64,
    pub(crate) return_asymmetry_fit: f64,
    pub(crate) aggregated_return_scale_fit: f64,
    pub(crate) business_time_return_scale_fit: f64,
    pub(crate) intraday_return_level_fit: f64,
    pub(crate) activity_shape_fit: f64,
    pub(crate) activity_level_fit: f64,
    pub(crate) rv_activity_coupling_fit: f64,
    pub(crate) price_span_sanity_fit: f64,
}

#[derive(Debug, Clone)]
struct JointRefinementRestartOutcomeGroup<'a> {
    parameter_bits: Vec<u64>,
    representative: &'a JointRefinementRestartOutcome,
    restart_outcomes: Vec<&'a JointRefinementRestartOutcome>,
}

#[derive(Debug, Clone, Copy)]
struct JointRefinementCandidateOrderingState<'a> {
    pareto_rank: usize,
    restart_outcome: &'a JointRefinementRestartOutcome,
    admissible: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementLazyCoarseConsistencySelection<'a> {
    pub(crate) selected_restart_outcomes: Vec<&'a JointRefinementRestartOutcome>,
    pub(crate) coarse_consistency_artifacts_by_restart_id:
        BTreeMap<String, JointRefinementCoarseConsistencyArtifacts>,
    pub(crate) used_unadmissible_fallback: bool,
}

#[derive(Debug, Default, Clone)]
struct LazyCoarseConsistencyCache {
    artifacts_by_restart_id: BTreeMap<String, JointRefinementCoarseConsistencyArtifacts>,
    artifacts_by_parameter_bits: BTreeMap<Vec<u64>, JointRefinementCoarseConsistencyArtifacts>,
}

#[derive(Debug, Default, Clone)]
struct LazyCoarseConsistencyProgressState {
    completed_units: usize,
    best_restart_id: Option<String>,
    best_weighted_objective: Option<f64>,
    best_coarse_consistency_fit: Option<f64>,
}

impl LazyCoarseConsistencyProgressState {
    fn record(
        &mut self,
        restart_id: &str,
        current_weighted_objective: f64,
        current_artifacts: Option<&JointRefinementCoarseConsistencyArtifacts>,
    ) -> Option<f64> {
        self.completed_units += 1;

        match self.best_weighted_objective {
            Some(current_best) if current_weighted_objective >= current_best => {}
            _ => {
                self.best_weighted_objective = Some(current_weighted_objective);
                self.best_restart_id = Some(restart_id.to_string());
            }
        }

        let current_coarse_consistency_fit = current_artifacts
            .as_ref()
            .map(|artifacts| artifacts.summary.normalized_fit);

        if let Some(artifacts) = current_artifacts {
            self.best_coarse_consistency_fit = Some(match self.best_coarse_consistency_fit {
                Some(current_best) => current_best.min(artifacts.summary.normalized_fit),
                None => artifacts.summary.normalized_fit,
            });
        }

        current_coarse_consistency_fit
    }
}

pub(crate) fn admissibility_threshold_from_sample_count(sample_count: usize) -> f64 {
    1.0 + (sample_count.max(1) as f64).recip().sqrt()
}

pub(crate) fn joint_breakdown_is_admissible(
    breakdown: &JointRefinementStageObjectiveBreakdown,
    thresholds: JointRefinementAdmissibilityThresholds,
    coarse_consistency_summary: Option<&JointRefinementCoarseConsistencySummary>,
    coarse_consistency_required: bool,
) -> bool {
    breakdown.aggregated_likelihood_fit <= thresholds.aggregated_likelihood_fit
        && breakdown.aggregated_return_shape_fit <= thresholds.aggregated_return_shape_fit
        && breakdown.return_asymmetry_fit <= thresholds.return_asymmetry_fit
        && breakdown.aggregated_return_scale_fit <= thresholds.aggregated_return_scale_fit
        && breakdown.business_time_return_scale_fit <= thresholds.business_time_return_scale_fit
        && breakdown.intraday_return_level_fit <= thresholds.intraday_return_level_fit
        && breakdown.activity_shape_fit <= thresholds.activity_shape_fit
        && breakdown.activity_level_fit <= thresholds.activity_level_fit
        && breakdown.rv_activity_coupling_fit <= thresholds.rv_activity_coupling_fit
        && breakdown.price_span_sanity_fit <= thresholds.price_span_sanity_fit
        && joint_coarse_consistency_is_admissible(
            coarse_consistency_summary,
            coarse_consistency_required,
        )
}

pub(crate) fn select_restart_outcomes_with_lazy_coarse_consistency<'a>(
    restart_outcomes: &'a [&'a JointRefinementRestartOutcome],
    context: &JointRefinementContext,
    baseline_artifacts: Option<&JointRefinementCoarseConsistencyArtifacts>,
    selected_restart_count: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> JointRefinementLazyCoarseConsistencySelection<'a> {
    let selected_restart_count = selected_restart_count.max(1);

    let Some(baseline_artifacts) = baseline_artifacts else {
        return JointRefinementLazyCoarseConsistencySelection {
            selected_restart_outcomes: select_restart_outcomes_without_admissibility(
                restart_outcomes,
                selected_restart_count,
            ),
            coarse_consistency_artifacts_by_restart_id: BTreeMap::new(),
            used_unadmissible_fallback: false,
        };
    };

    let restart_groups = build_joint_restart_outcome_groups(restart_outcomes);

    let representative_restart_outcomes =
        representative_restart_outcomes(restart_groups.as_slice());

    let optimistic_thresholds = joint_refinement_admissibility_thresholds(context);

    let optimistic_coarse_consistency_by_restart_id =
        BTreeMap::<String, JointRefinementCoarseConsistencySummary>::new();

    let optimistic_candidate_states = build_joint_candidate_ordering_states(
        representative_restart_outcomes.as_slice(),
        optimistic_thresholds,
        &optimistic_coarse_consistency_by_restart_id,
        false,
    );

    let has_optimistically_admissible_candidate = optimistic_candidate_states
        .iter()
        .any(|candidate_state| candidate_state.admissible);

    let mut ranked_restart_candidates = optimistic_candidate_states
        .iter()
        .filter(|candidate_state| candidate_state.admissible)
        .copied()
        .collect::<Vec<_>>();

    ranked_restart_candidates.sort_by(|left, right| {
        compare_joint_candidate_ordering_states(
            *left,
            *right,
            has_optimistically_admissible_candidate,
        )
    });

    let total_ranked_restart_groups = ranked_restart_candidates.len();

    let mut coarse_consistency_cache = LazyCoarseConsistencyCache::default();

    seed_baseline_coarse_consistency(
        &mut coarse_consistency_cache,
        restart_groups.as_slice(),
        baseline_artifacts,
    );

    emit_coarse_consistency_progress(
        progress_sender,
        0,
        total_ranked_restart_groups,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let mut progress_state = LazyCoarseConsistencyProgressState::default();
    let mut selected_restart_outcomes = Vec::new();

    for candidate_state in ranked_restart_candidates {
        let Some(restart_group) = find_restart_group(
            restart_groups.as_slice(),
            candidate_state.restart_outcome.restart_id.as_str(),
        ) else {
            continue;
        };

        let current_weighted_objective = candidate_state
            .restart_outcome
            .final_evaluation
            .breakdown
            .weighted_objective;

        let current_artifacts = resolve_restart_group_coarse_consistency(
            &mut coarse_consistency_cache,
            context,
            restart_group,
            baseline_artifacts,
        );

        let current_coarse_consistency_fit =
            progress_state.record(
                candidate_state.restart_outcome.restart_id.as_str(),
                current_weighted_objective,
                current_artifacts.as_ref(),
            );

        emit_coarse_consistency_progress(
            progress_sender,
            progress_state.completed_units,
            total_ranked_restart_groups,
            Some(candidate_state.restart_outcome.restart_id.as_str()),
            Some(current_weighted_objective),
            progress_state.best_restart_id.as_deref(),
            progress_state.best_weighted_objective,
            current_coarse_consistency_fit,
            progress_state.best_coarse_consistency_fit,
        );

        if current_artifacts
            .as_ref()
            .is_some_and(|artifacts| coarse_consistency_summary_is_admissible(&artifacts.summary))
        {
            if let Some(selected_restart_outcome) = find_restart_outcome(
                restart_outcomes,
                restart_group.representative.restart_id.as_str(),
            ) {
                selected_restart_outcomes.push(selected_restart_outcome);

                if selected_restart_outcomes.len() >= selected_restart_count {
                    return JointRefinementLazyCoarseConsistencySelection {
                        selected_restart_outcomes,
                        coarse_consistency_artifacts_by_restart_id: coarse_consistency_cache
                            .artifacts_by_restart_id,
                        used_unadmissible_fallback: false,
                    };
                }
            }
        }
    }

    let mut used_unadmissible_fallback = false;
    if selected_restart_outcomes.is_empty() {
        selected_restart_outcomes = select_restart_outcomes_without_admissibility(
            representative_restart_outcomes.as_slice(),
            1,
        );
        used_unadmissible_fallback = true;
    }

    for selected_restart_outcome in &selected_restart_outcomes {
        ensure_selected_restart_artifacts(
            &mut coarse_consistency_cache,
            context,
            restart_groups.as_slice(),
            Some(selected_restart_outcome),
            baseline_artifacts,
        );
    }

    JointRefinementLazyCoarseConsistencySelection {
        selected_restart_outcomes,
        coarse_consistency_artifacts_by_restart_id: coarse_consistency_cache
            .artifacts_by_restart_id,
        used_unadmissible_fallback,
    }
}

fn representative_restart_outcomes<'a>(
    restart_groups: &[JointRefinementRestartOutcomeGroup<'a>],
) -> Vec<&'a JointRefinementRestartOutcome> {
    restart_groups
        .iter()
        .map(|restart_group| restart_group.representative)
        .collect()
}

fn find_restart_group<'a>(
    restart_groups: &'a [JointRefinementRestartOutcomeGroup<'a>],
    restart_id: &str,
) -> Option<&'a JointRefinementRestartOutcomeGroup<'a>> {
    restart_groups
        .iter()
        .find(|restart_group| restart_group.representative.restart_id == restart_id)
}

fn find_restart_outcome<'a>(
    restart_outcomes: &[&'a JointRefinementRestartOutcome],
    restart_id: &str,
) -> Option<&'a JointRefinementRestartOutcome> {
    restart_outcomes
        .iter()
        .copied()
        .find(|restart_outcome| restart_outcome.restart_id == restart_id)
}

fn seed_baseline_coarse_consistency(
    coarse_consistency_cache: &mut LazyCoarseConsistencyCache,
    restart_groups: &[JointRefinementRestartOutcomeGroup<'_>],
    baseline_artifacts: &JointRefinementCoarseConsistencyArtifacts,
) {
    for restart_group in restart_groups {
        if restart_group
            .restart_outcomes
            .iter()
            .any(|restart_outcome| restart_outcome.restart_id == "baseline")
        {
            coarse_consistency_cache.artifacts_by_parameter_bits.insert(
                restart_group.parameter_bits.clone(),
                baseline_artifacts.clone(),
            );

            assign_coarse_consistency_artifacts_to_restart_group(
                &mut coarse_consistency_cache.artifacts_by_restart_id,
                restart_group,
                baseline_artifacts,
            );
        }
    }
}

fn resolve_restart_group_coarse_consistency(
    coarse_consistency_cache: &mut LazyCoarseConsistencyCache,
    context: &JointRefinementContext,
    restart_group: &JointRefinementRestartOutcomeGroup<'_>,
    baseline_artifacts: &JointRefinementCoarseConsistencyArtifacts,
) -> Option<JointRefinementCoarseConsistencyArtifacts> {
    if let Some(artifacts) = coarse_consistency_cache
        .artifacts_by_parameter_bits
        .get(&restart_group.parameter_bits)
        .cloned()
    {
        assign_coarse_consistency_artifacts_to_restart_group(
            &mut coarse_consistency_cache.artifacts_by_restart_id,
            restart_group,
            &artifacts,
        );

        return Some(artifacts);
    }

    let resolved_artifacts = build_joint_candidate_coarse_consistency_artifacts(
        context,
        &restart_group
            .representative
            .final_evaluation
            .fitted_parameters,
        Some(restart_group.parameter_bits.as_slice()),
        Some(baseline_artifacts.summary.raw_fit),
    )?;

    coarse_consistency_cache.artifacts_by_parameter_bits.insert(
        restart_group.parameter_bits.clone(),
        resolved_artifacts.clone(),
    );

    assign_coarse_consistency_artifacts_to_restart_group(
        &mut coarse_consistency_cache.artifacts_by_restart_id,
        restart_group,
        &resolved_artifacts,
    );

    Some(resolved_artifacts)
}

fn ensure_selected_restart_artifacts(
    coarse_consistency_cache: &mut LazyCoarseConsistencyCache,
    context: &JointRefinementContext,
    restart_groups: &[JointRefinementRestartOutcomeGroup<'_>],
    selected_restart_outcome: Option<&JointRefinementRestartOutcome>,
    baseline_artifacts: &JointRefinementCoarseConsistencyArtifacts,
) {
    let Some(selected_restart_outcome) = selected_restart_outcome else {
        return;
    };

    let Some(selected_restart_group) =
        find_restart_group(restart_groups, selected_restart_outcome.restart_id.as_str())
    else {
        return;
    };

    if coarse_consistency_cache
        .artifacts_by_restart_id
        .contains_key(selected_restart_outcome.restart_id.as_str())
    {
        return;
    }

    if let Some(selected_artifacts) = build_joint_candidate_coarse_consistency_artifacts(
        context,
        &selected_restart_outcome.final_evaluation.fitted_parameters,
        Some(selected_restart_group.parameter_bits.as_slice()),
        Some(baseline_artifacts.summary.raw_fit),
    ) {
        assign_coarse_consistency_artifacts_to_restart_group(
            &mut coarse_consistency_cache.artifacts_by_restart_id,
            selected_restart_group,
            &selected_artifacts,
        );
    }
}

fn coarse_consistency_summary_is_admissible(
    summary: &JointRefinementCoarseConsistencySummary,
) -> bool {
    summary.normalized_fit <= summary.admissibility_threshold
}

pub(crate) fn build_joint_candidate_frontier_diagnostics(
    restart_outcomes: &[JointRefinementRestartOutcome],
    context: &JointRefinementContext,
    coarse_consistency_by_restart_id: &BTreeMap<String, JointRefinementCoarseConsistencySummary>,
    coarse_consistency_required: bool,
) -> Vec<JointRefinementStageCandidateDiagnostic> {
    if restart_outcomes.is_empty() {
        return Vec::new();
    }

    let thresholds = joint_refinement_admissibility_thresholds(context);

    let restart_outcome_refs = restart_outcomes.iter().collect::<Vec<_>>();

    let candidate_states = build_joint_candidate_ordering_states(
        restart_outcome_refs.as_slice(),
        thresholds,
        coarse_consistency_by_restart_id,
        coarse_consistency_required,
    );

    let has_admissible_candidate = candidate_states
        .iter()
        .any(|candidate_state| candidate_state.admissible);

    let mut ranked_candidates = candidate_states
        .iter()
        .map(|candidate_state| (*candidate_state, candidate_state.restart_outcome))
        .collect::<Vec<_>>();

    ranked_candidates.sort_by(|left, right| {
        compare_joint_candidate_ordering_states(left.0, right.0, has_admissible_candidate)
    });

    let lexicographic_rank_by_restart_id: BTreeMap<&str, usize> = ranked_candidates
        .iter()
        .enumerate()
        .map(|(position, (_, outcome))| (outcome.restart_id.as_str(), position + 1))
        .collect();

    restart_outcomes
        .iter()
        .map(|outcome| {
            let candidate_state = candidate_states
                .iter()
                .find(|candidate_state| {
                    candidate_state.restart_outcome.restart_id == outcome.restart_id
                })
                .copied()
                .unwrap_or(JointRefinementCandidateOrderingState {
                    pareto_rank: usize::MAX,
                    restart_outcome: outcome,
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
                candidate_id: format!("joint_refinement_{}", outcome.restart_id),
                valid,
                admissible: candidate_state.admissible,
                pareto_rank: candidate_state.pareto_rank,
                lexicographic_rank: *lexicographic_rank_by_restart_id
                    .get(outcome.restart_id.as_str())
                    .unwrap_or(&(restart_outcomes.len() + 1)),
                scalar_tie_break: outcome.final_evaluation.breakdown.weighted_objective,
                local_evaluation_count: outcome.local_evaluation_count,
                stop_reason: outcome.stop_reason.to_string(),
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

fn joint_refinement_admissibility_thresholds(
    context: &JointRefinementContext,
) -> JointRefinementAdmissibilityThresholds {
    let fifteen_minute_sample_count = context.observed_data.observed_15m_intervals.len();
    let hourly_sample_count = context.observed_data.observed_hourly_tick_counts.len();
    let rv_sample_count = context.observed_data.observed_15m_intervals.len();

    JointRefinementAdmissibilityThresholds {
        aggregated_likelihood_fit: admissibility_threshold_from_sample_count(
            fifteen_minute_sample_count,
        ),
        aggregated_return_shape_fit: admissibility_threshold_from_sample_count(
            fifteen_minute_sample_count,
        ),
        return_asymmetry_fit: admissibility_threshold_from_sample_count(
            fifteen_minute_sample_count,
        ),
        aggregated_return_scale_fit: admissibility_threshold_from_sample_count(
            fifteen_minute_sample_count,
        ),
        business_time_return_scale_fit: admissibility_threshold_from_sample_count(
            fifteen_minute_sample_count / 4,
        ),
        intraday_return_level_fit: admissibility_threshold_from_sample_count(hourly_sample_count),
        activity_shape_fit: admissibility_threshold_from_sample_count(hourly_sample_count),
        activity_level_fit: admissibility_threshold_from_sample_count(hourly_sample_count),
        rv_activity_coupling_fit: admissibility_threshold_from_sample_count(rv_sample_count),
        price_span_sanity_fit: admissibility_threshold_from_sample_count(
            fifteen_minute_sample_count / 8,
        ),
    }
}

fn joint_restart_outcome_is_admissible(
    restart_outcome: &JointRefinementRestartOutcome,
    thresholds: JointRefinementAdmissibilityThresholds,
    coarse_consistency_summary: Option<&JointRefinementCoarseConsistencySummary>,
    coarse_consistency_required: bool,
) -> bool {
    let breakdown = &restart_outcome.final_evaluation.breakdown;

    joint_breakdown_is_admissible(
        breakdown,
        thresholds,
        coarse_consistency_summary,
        coarse_consistency_required,
    )
}

fn joint_coarse_consistency_is_admissible(
    coarse_consistency_summary: Option<&JointRefinementCoarseConsistencySummary>,
    coarse_consistency_required: bool,
) -> bool {
    match coarse_consistency_summary {
        Some(summary) => summary.normalized_fit <= summary.admissibility_threshold,
        None => !coarse_consistency_required,
    }
}

fn joint_restart_outcome_dominates(
    left: &JointRefinementRestartOutcome,
    right: &JointRefinementRestartOutcome,
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

fn build_joint_restart_outcome_groups<'a>(
    restart_outcomes: &[&'a JointRefinementRestartOutcome],
) -> Vec<JointRefinementRestartOutcomeGroup<'a>> {
    let grouped_restart_outcomes = restart_outcomes.iter().copied().fold(
        BTreeMap::<Vec<u64>, Vec<&JointRefinementRestartOutcome>>::new(),
        |mut grouped_restart_outcomes, restart_outcome| {
            grouped_restart_outcomes
                .entry(ordered_parameter_bits(
                    restart_outcome.parameter_vector.as_slice(),
                ))
                .or_default()
                .push(restart_outcome);
            grouped_restart_outcomes
        },
    );

    grouped_restart_outcomes
        .into_iter()
        .filter_map(|(parameter_bits, grouped_restart_outcomes)| {
            let representative = grouped_restart_outcomes
                .iter()
                .copied()
                .min_by(|left, right| left.restart_id.cmp(&right.restart_id))?;
            Some(JointRefinementRestartOutcomeGroup {
                parameter_bits,
                representative,
                restart_outcomes: grouped_restart_outcomes,
            })
        })
        .collect()
}

fn assign_coarse_consistency_artifacts_to_restart_group(
    coarse_consistency_artifacts_by_restart_id: &mut BTreeMap<
        String,
        JointRefinementCoarseConsistencyArtifacts,
    >,
    restart_group: &JointRefinementRestartOutcomeGroup<'_>,
    artifacts: &JointRefinementCoarseConsistencyArtifacts,
) {
    for restart_outcome in &restart_group.restart_outcomes {
        coarse_consistency_artifacts_by_restart_id
            .insert(restart_outcome.restart_id.clone(), artifacts.clone());
    }
}

fn select_restart_outcomes_without_admissibility<'a>(
    restart_outcomes: &[&'a JointRefinementRestartOutcome],
    selected_restart_count: usize,
) -> Vec<&'a JointRefinementRestartOutcome> {
    if restart_outcomes.is_empty() {
        return Vec::new();
    }

    let pareto_ranks = pareto_ranks(restart_outcomes);

    let mut ranked_candidates = restart_outcomes
        .iter()
        .enumerate()
        .map(|(position, restart_outcome)| (pareto_ranks[position], *restart_outcome))
        .collect::<Vec<_>>();

    ranked_candidates.sort_by(|left, right| {
        compare_joint_candidate_breakdowns(
            left.0,
            &left.1.final_evaluation.breakdown,
            false,
            left.1.restart_id.as_str(),
            right.0,
            &right.1.final_evaluation.breakdown,
            false,
            right.1.restart_id.as_str(),
            false,
        )
    });

    ranked_candidates
        .into_iter()
        .take(selected_restart_count.max(1))
        .map(|(_, outcome)| outcome)
        .collect()
}

fn build_joint_candidate_ordering_states<'a>(
    restart_outcomes: &'a [&'a JointRefinementRestartOutcome],
    thresholds: JointRefinementAdmissibilityThresholds,
    coarse_consistency_by_restart_id: &BTreeMap<String, JointRefinementCoarseConsistencySummary>,
    coarse_consistency_required: bool,
) -> Vec<JointRefinementCandidateOrderingState<'a>> {
    let pareto_ranks = pareto_ranks(restart_outcomes);

    restart_outcomes
        .iter()
        .enumerate()
        .map(
            |(position, outcome)| JointRefinementCandidateOrderingState {
                pareto_rank: pareto_ranks[position],
                restart_outcome: outcome,
                admissible: outcome.local_evaluation.invalid_reason.is_none()
                    && outcome.final_evaluation.invalid_reason.is_none()
                    && joint_restart_outcome_is_admissible(
                        outcome,
                        thresholds,
                        coarse_consistency_by_restart_id.get(outcome.restart_id.as_str()),
                        coarse_consistency_required,
                    ),
            },
        )
        .collect()
}

fn pareto_ranks(restart_outcomes: &[&JointRefinementRestartOutcome]) -> Vec<usize> {
    restart_outcomes
        .iter()
        .map(|candidate| {
            1 + restart_outcomes
                .iter()
                .filter(|other| joint_restart_outcome_dominates(other, candidate))
                .count()
        })
        .collect()
}

fn compare_joint_candidate_ordering_states(
    left: JointRefinementCandidateOrderingState<'_>,
    right: JointRefinementCandidateOrderingState<'_>,
    has_admissible_candidate: bool,
) -> std::cmp::Ordering {
    compare_joint_candidate_breakdowns(
        left.pareto_rank,
        &left.restart_outcome.final_evaluation.breakdown,
        left.admissible,
        left.restart_outcome.restart_id.as_str(),
        right.pareto_rank,
        &right.restart_outcome.final_evaluation.breakdown,
        right.admissible,
        right.restart_outcome.restart_id.as_str(),
        has_admissible_candidate,
    )
}

fn named_joint_parameter_coordinates(
    parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
) -> Vec<(String, f64)> {
    let mut coordinates = vec![
        (
            active_parameter_layout.log_lambda_scale_index,
            "log_lambda_scale".to_string(),
        ),
        (
            active_parameter_layout.log_beta_scale_index,
            "log_beta_scale".to_string(),
        ),
        (
            active_parameter_layout.log_self_excitation_scale_index,
            "log_self_excitation_scale".to_string(),
        ),
        (
            active_parameter_layout.log_cross_excitation_scale_index,
            "log_cross_excitation_scale".to_string(),
        ),
        (
            active_parameter_layout.nig_block[0],
            "nig.log_alpha".to_string(),
        ),
        (
            active_parameter_layout.nig_block[1],
            "nig.buy_skew_coordinate".to_string(),
        ),
        (
            active_parameter_layout.nig_block[2],
            "nig.sell_relative_coordinate".to_string(),
        ),
        (
            active_parameter_layout.nig_block[3],
            "nig.log_delta_offset".to_string(),
        ),
    ];

    if let Some(index) = active_parameter_layout.known_activity_scale_index {
        coordinates.push((index, "known_activity_scale".to_string()));
    }
    if let Some(index) = active_parameter_layout.surprise_activity_scale_index {
        coordinates.push((index, "surprise_activity_scale".to_string()));
    }
    if let Some(index) = active_parameter_layout.surprise_baseline_scale_index {
        coordinates.push((index, "surprise_baseline_scale".to_string()));
    }

    coordinates.sort_by_key(|(index, _)| *index);

    coordinates
        .into_iter()
        .filter_map(|(index, label)| {
            parameter_vector
                .get(index)
                .copied()
                .map(|value| (label, value))
        })
        .collect()
}

fn emit_coarse_consistency_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
    current_weighted_objective: Option<f64>,
    best_restart_id: Option<&str>,
    best_weighted_objective: Option<f64>,
    current_coarse_consistency_fit: Option<f64>,
    best_coarse_consistency_fit: Option<f64>,
) {
    if let Some(progress_sender) = progress_sender {
        let _ = progress_sender.send(JointRefinementStageProgressUpdate {
            current_item_label: current_item_label.map(str::to_string),
            current_weighted_objective,
            best_restart_id: best_restart_id.map(str::to_string),
            best_weighted_objective,
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
