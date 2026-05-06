use std::collections::BTreeMap;
use std::sync::mpsc::Sender;

use crate::stages::return_components::continuous_return_calibration_sample_observed_returns;

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
    JointRefinementActiveParameterLayout, JointRefinementCoarseConsistencySummary,
    JointRefinementContext, JointRefinementRawAdmissibilityMetrics, JointRefinementRestartOutcome,
    ordered_parameter_bits,
};

pub(crate) const JOINT_REFINEMENT_THRESHOLD_RELATIVE_ADMISSIBILITY_SCORE: f64 = 1.0;

#[derive(Debug, Clone, Copy)]
pub(crate) struct JointRefinementAdmissibilityThresholds {
    pub(crate) aggregated_likelihood_fit: f64,
    pub(crate) aggregated_return_shape_fit: f64,
    pub(crate) return_asymmetry_fit: f64,
    pub(crate) tick_return_asymmetry_fit: f64,
    pub(crate) aggregated_return_scale_fit: f64,
    pub(crate) tick_return_scale_fit: f64,
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

fn baseline_relative_admissibility_threshold(
    local_baseline_bucket: f64,
    final_baseline_bucket: f64,
    sample_count: usize,
) -> f64 {
    let unit_fit_reference = 1.0;
    let baseline_reference = [
        local_baseline_bucket,
        final_baseline_bucket,
        unit_fit_reference,
    ]
    .into_iter()
    .filter(|value| value.is_finite() && *value > 0.0)
    .fold(unit_fit_reference, f64::max);

    baseline_reference * admissibility_threshold_from_sample_count(sample_count)
}

fn likelihood_admissibility_threshold(
    local_baseline_score: f64,
    final_baseline_score: f64,
    sample_count: usize,
) -> f64 {
    let baseline_reference = [local_baseline_score, final_baseline_score]
        .into_iter()
        .filter(|value| value.is_finite())
        .fold(f64::NEG_INFINITY, f64::max);

    if !baseline_reference.is_finite() {
        return f64::INFINITY;
    }

    // Average NLL differences are log likelihood ratios, so the sample-count
    // admissibility band is additive in the raw likelihood score.
    baseline_reference + admissibility_threshold_from_sample_count(sample_count).ln()
}

pub(crate) fn joint_raw_metrics_are_admissible(
    raw_metrics: &JointRefinementRawAdmissibilityMetrics,
    thresholds: JointRefinementAdmissibilityThresholds,
    coarse_consistency_summary: Option<&JointRefinementCoarseConsistencySummary>,
    coarse_consistency_required: bool,
) -> bool {
    raw_metrics.aggregated_likelihood_fit <= thresholds.aggregated_likelihood_fit
        && raw_metrics.aggregated_return_shape_fit <= thresholds.aggregated_return_shape_fit
        && raw_metrics.return_asymmetry_fit <= thresholds.return_asymmetry_fit
        && raw_metrics.tick_return_asymmetry_fit <= thresholds.tick_return_asymmetry_fit
        && raw_metrics.aggregated_return_scale_fit <= thresholds.aggregated_return_scale_fit
        && raw_metrics.tick_return_scale_fit <= thresholds.tick_return_scale_fit
        && raw_metrics.business_time_return_scale_fit <= thresholds.business_time_return_scale_fit
        && raw_metrics.intraday_return_level_fit <= thresholds.intraday_return_level_fit
        && raw_metrics.activity_shape_fit <= thresholds.activity_shape_fit
        && raw_metrics.activity_level_fit <= thresholds.activity_level_fit
        && raw_metrics.rv_activity_coupling_fit <= thresholds.rv_activity_coupling_fit
        && raw_metrics.price_span_sanity_fit <= thresholds.price_span_sanity_fit
        && joint_coarse_consistency_is_admissible(
            coarse_consistency_summary,
            coarse_consistency_required,
        )
}

pub(crate) fn joint_raw_metrics_are_dynamics_feasible(
    raw_metrics: &JointRefinementRawAdmissibilityMetrics,
    thresholds: JointRefinementAdmissibilityThresholds,
) -> bool {
    raw_metrics.aggregated_likelihood_fit <= thresholds.aggregated_likelihood_fit
        && raw_metrics.business_time_return_scale_fit <= thresholds.business_time_return_scale_fit
        && raw_metrics.intraday_return_level_fit <= thresholds.intraday_return_level_fit
        && raw_metrics.activity_shape_fit <= thresholds.activity_shape_fit
        && raw_metrics.activity_level_fit <= thresholds.activity_level_fit
        && raw_metrics.rv_activity_coupling_fit <= thresholds.rv_activity_coupling_fit
        && raw_metrics.price_span_sanity_fit <= thresholds.price_span_sanity_fit
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
            selected_restart_outcomes: Vec::new(),
            coarse_consistency_artifacts_by_restart_id: BTreeMap::new(),
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

    emit_coarse_consistency_progress(CoarseConsistencyProgressEmissionInput {
        progress_sender,
        completed_units: 0,
        total_units: total_ranked_restart_groups,
        current_item_label: None,
        current_weighted_objective: None,
        best_restart_id: None,
        best_weighted_objective: None,
        current_coarse_consistency_fit: None,
        best_coarse_consistency_fit: None,
    });

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

        emit_coarse_consistency_progress(CoarseConsistencyProgressEmissionInput {
            progress_sender,
            completed_units: progress_state.completed_units,
            total_units: total_ranked_restart_groups,
            current_item_label: Some(candidate_state.restart_outcome.restart_id.as_str()),
            current_weighted_objective: Some(current_weighted_objective),
            best_restart_id: progress_state.best_restart_id.as_deref(),
            best_weighted_objective: progress_state.best_weighted_objective,
            current_coarse_consistency_fit: None,
            best_coarse_consistency_fit: progress_state.best_coarse_consistency_fit,
        });

        let current_artifacts = resolve_restart_group_coarse_consistency(
            &mut coarse_consistency_cache,
            context,
            restart_group,
            baseline_artifacts,
            progress_sender,
        );

        let current_coarse_consistency_fit = progress_state.record(
            candidate_state.restart_outcome.restart_id.as_str(),
            current_weighted_objective,
            current_artifacts.as_ref(),
        );

        emit_coarse_consistency_progress(CoarseConsistencyProgressEmissionInput {
            progress_sender,
            completed_units: progress_state.completed_units,
            total_units: total_ranked_restart_groups,
            current_item_label: Some(candidate_state.restart_outcome.restart_id.as_str()),
            current_weighted_objective: Some(current_weighted_objective),
            best_restart_id: progress_state.best_restart_id.as_deref(),
            best_weighted_objective: progress_state.best_weighted_objective,
            current_coarse_consistency_fit,
            best_coarse_consistency_fit: progress_state.best_coarse_consistency_fit,
        });

        if current_artifacts
            .as_ref()
            .is_some_and(|artifacts| coarse_consistency_summary_is_admissible(&artifacts.summary))
            && let Some(selected_restart_outcome) = find_restart_outcome(
                restart_outcomes,
                restart_group.representative.restart_id.as_str(),
            )
        {
            selected_restart_outcomes.push(selected_restart_outcome);

            if selected_restart_outcomes.len() >= selected_restart_count {
                return JointRefinementLazyCoarseConsistencySelection {
                    selected_restart_outcomes,
                    coarse_consistency_artifacts_by_restart_id: coarse_consistency_cache
                        .artifacts_by_restart_id,
                };
            }
        }
    }

    for selected_restart_outcome in &selected_restart_outcomes {
        ensure_selected_restart_artifacts(
            &mut coarse_consistency_cache,
            context,
            restart_groups.as_slice(),
            Some(selected_restart_outcome),
            baseline_artifacts,
            progress_sender,
        );
    }

    JointRefinementLazyCoarseConsistencySelection {
        selected_restart_outcomes,
        coarse_consistency_artifacts_by_restart_id: coarse_consistency_cache
            .artifacts_by_restart_id,
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
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
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
        Some(JointRefinementCoarseConsistencyProgress {
            sender: progress_sender,
            phase: JointRefinementStageProgressPhase::CoarseConsistencyEvaluation,
            current_item_label: Some(restart_group.representative.restart_id.as_str()),
        }),
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
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
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
        Some(JointRefinementCoarseConsistencyProgress {
            sender: progress_sender,
            phase: JointRefinementStageProgressPhase::CoarseConsistencyEvaluation,
            current_item_label: Some(selected_restart_outcome.restart_id.as_str()),
        }),
    ) {
        assign_coarse_consistency_artifacts_to_restart_group(
            &mut coarse_consistency_cache.artifacts_by_restart_id,
            selected_restart_group,
            &selected_artifacts,
        );
    }
}

pub(crate) fn coarse_consistency_summary_is_admissible(
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

pub(crate) fn joint_refinement_admissibility_thresholds(
    context: &JointRefinementContext,
) -> JointRefinementAdmissibilityThresholds {
    let fifteen_minute_sample_count = context.observed_data.observed_15m_intervals.len();
    let hourly_sample_count = context.observed_data.observed_hourly_tick_counts.len();
    let rv_sample_count = context.observed_data.observed_15m_intervals.len();
    let observed_tick_sample_count = continuous_return_calibration_sample_observed_returns(
        &context
            .observed_data
            .observed_tick_continuous_return_calibration_sample,
    )
    .len();

    JointRefinementAdmissibilityThresholds {
        aggregated_likelihood_fit: likelihood_admissibility_threshold(
            context.local_baseline_buckets.aggregated_likelihood_fit,
            context.final_baseline_buckets.aggregated_likelihood_fit,
            fifteen_minute_sample_count,
        ),
        aggregated_return_shape_fit: baseline_relative_admissibility_threshold(
            context.local_baseline_buckets.aggregated_return_shape_fit,
            context.final_baseline_buckets.aggregated_return_shape_fit,
            fifteen_minute_sample_count,
        ),
        return_asymmetry_fit: baseline_relative_admissibility_threshold(
            context.local_baseline_buckets.return_asymmetry_fit,
            context.final_baseline_buckets.return_asymmetry_fit,
            fifteen_minute_sample_count,
        ),
        tick_return_asymmetry_fit: baseline_relative_admissibility_threshold(
            context.local_baseline_buckets.tick_return_asymmetry_fit,
            context.final_baseline_buckets.tick_return_asymmetry_fit,
            observed_tick_sample_count,
        ),
        aggregated_return_scale_fit: baseline_relative_admissibility_threshold(
            context.local_baseline_buckets.aggregated_return_scale_fit,
            context.final_baseline_buckets.aggregated_return_scale_fit,
            fifteen_minute_sample_count,
        ),
        tick_return_scale_fit: baseline_relative_admissibility_threshold(
            context.local_baseline_buckets.tick_return_scale_fit,
            context.final_baseline_buckets.tick_return_scale_fit,
            observed_tick_sample_count,
        ),
        business_time_return_scale_fit: baseline_relative_admissibility_threshold(
            context
                .local_baseline_buckets
                .business_time_return_scale_fit,
            context
                .final_baseline_buckets
                .business_time_return_scale_fit,
            fifteen_minute_sample_count / 4,
        ),
        intraday_return_level_fit: baseline_relative_admissibility_threshold(
            context.local_baseline_buckets.intraday_return_level_fit,
            context.final_baseline_buckets.intraday_return_level_fit,
            hourly_sample_count,
        ),
        activity_shape_fit: baseline_relative_admissibility_threshold(
            context.local_baseline_buckets.activity_shape_fit,
            context.final_baseline_buckets.activity_shape_fit,
            hourly_sample_count,
        ),
        activity_level_fit: baseline_relative_admissibility_threshold(
            context.local_baseline_buckets.activity_level_fit,
            context.final_baseline_buckets.activity_level_fit,
            hourly_sample_count,
        ),
        rv_activity_coupling_fit: baseline_relative_admissibility_threshold(
            context.local_baseline_buckets.rv_activity_coupling_fit,
            context.final_baseline_buckets.rv_activity_coupling_fit,
            rv_sample_count,
        ),
        price_span_sanity_fit: baseline_relative_admissibility_threshold(
            context.local_baseline_buckets.price_span_sanity_fit,
            context.final_baseline_buckets.price_span_sanity_fit,
            fifteen_minute_sample_count / 8,
        ),
    }
}

pub(crate) fn joint_restart_outcome_is_admissible(
    restart_outcome: &JointRefinementRestartOutcome,
    thresholds: JointRefinementAdmissibilityThresholds,
    coarse_consistency_summary: Option<&JointRefinementCoarseConsistencySummary>,
    coarse_consistency_required: bool,
) -> bool {
    joint_raw_metrics_are_admissible(
        &restart_outcome.final_evaluation.raw_admissibility_metrics,
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
    compare_joint_candidate_breakdowns(JointCandidateBreakdownComparisonInput {
        left_pareto_rank: left.pareto_rank,
        left_breakdown: &left.restart_outcome.final_evaluation.breakdown,
        left_admissible: left.admissible,
        left_restart_id: left.restart_outcome.restart_id.as_str(),
        right_pareto_rank: right.pareto_rank,
        right_breakdown: &right.restart_outcome.final_evaluation.breakdown,
        right_admissible: right.admissible,
        right_restart_id: right.restart_outcome.restart_id.as_str(),
        has_admissible_candidate,
    })
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

struct CoarseConsistencyProgressEmissionInput<'a> {
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&'a str>,
    current_weighted_objective: Option<f64>,
    best_restart_id: Option<&'a str>,
    best_weighted_objective: Option<f64>,
    current_coarse_consistency_fit: Option<f64>,
    best_coarse_consistency_fit: Option<f64>,
}

fn emit_coarse_consistency_progress(input: CoarseConsistencyProgressEmissionInput<'_>) {
    let CoarseConsistencyProgressEmissionInput {
        progress_sender,
        completed_units,
        total_units,
        current_item_label,
        current_weighted_objective,
        best_restart_id,
        best_weighted_objective,
        current_coarse_consistency_fit,
        best_coarse_consistency_fit,
    } = input;

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

#[cfg(test)]
mod tests {
    use super::{admissibility_threshold_from_sample_count, likelihood_admissibility_threshold};

    #[test]
    fn likelihood_admissibility_threshold_uses_log_margin_above_baseline_score() {
        let sample_count = 400;
        let baseline_score = -0.5;

        let threshold =
            likelihood_admissibility_threshold(baseline_score, baseline_score - 0.25, sample_count);

        assert_eq!(
            threshold,
            baseline_score + admissibility_threshold_from_sample_count(sample_count).ln()
        );
    }
}
