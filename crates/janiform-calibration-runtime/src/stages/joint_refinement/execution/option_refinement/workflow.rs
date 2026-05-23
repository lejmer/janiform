use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::time::Instant;

use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, JointRefinementStageOptionSearchSelectionAgreementDiagnostics,
};
use janiform_engine_pricing::internal::SpectralThetaWarmStartState;
use janiform_foundations::numeric::pearson_correlation;
use janiform_work_scheduler::{StaticWorkPoolInput, run_static_work_pool};

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate,
};
use crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTierKind;

use super::super::super::setup::parameter_vector_to_fitted_parameters;
use super::super::{
    JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND, JointRefinementContext,
    JointRefinementOptionTierPricingArtifact, JointRefinementRestartOutcome,
    JointRefinementResumeStore, OptionPricingProgressInput, OptionPricingProgressSnapshot,
    OptionPricingProgressSnapshotSource, OptionSurfaceTierPricingArtifactResolutionInput,
    elapsed_millis, ordered_parameter_bits, planned_restart_work_item_labels,
    resolve_option_surface_tier_pricing_artifact,
};
use super::candidate_parallelism::{
    option_refinement_candidate_parallelism, option_refinement_candidate_pricing_cpu_counts,
};
use super::pricing_block_optimizer::{
    OptionRefinementCandidateSearchBudget, OptionRefinementCandidateWallClockGuard,
    optimize_pricing_block_from_search_tier_sensitivities,
};
use super::reduced_model::option_reduced_model_parameter_count;

pub(crate) const JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT: usize = 5;

#[derive(Debug)]
pub(super) struct OptionRefinementProgressContext<'a> {
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    phase: JointRefinementStageProgressPhase,
    total_units: usize,
    current_restart_id: &'a str,
    shared_progress_state: Arc<OptionRefinementSharedProgressState>,
    current_evaluation_count: Cell<usize>,
    current_weighted_objective: Cell<Option<f64>>,
    total_evaluation_budget: Option<usize>,
}

impl OptionRefinementProgressContext<'_> {
    fn record_evaluation_started(&self) {
        self.current_evaluation_count
            .set(self.current_evaluation_count.get().saturating_add(1));
    }

    pub(super) fn current_evaluation_count(&self) -> usize {
        self.current_evaluation_count.get()
    }

    fn record_weighted_objective(&self, weighted_objective: f64) {
        self.current_weighted_objective
            .set(Some(weighted_objective));
        self.shared_progress_state
            .record_candidate_score(self.current_restart_id, weighted_objective);
    }

    fn current_weighted_objective(&self) -> Option<f64> {
        self.current_weighted_objective.get()
    }

    pub(super) const fn total_evaluation_budget(&self) -> Option<usize> {
        self.total_evaluation_budget
    }

    fn running_pricing_progress_percent(
        &self,
        pricing_event_progress_percent: Option<usize>,
    ) -> usize {
        self.running_candidate_progress_percent(pricing_event_progress_percent)
            .unwrap_or_else(|| pricing_event_progress_percent.unwrap_or(0).min(99))
    }

    fn running_candidate_progress_percent(
        &self,
        pricing_event_progress_percent: Option<usize>,
    ) -> Option<usize> {
        let completed_evaluation_count = self.current_evaluation_count().saturating_sub(1);
        let current_evaluation_progress_percent = pricing_event_progress_percent.unwrap_or(0);

        self.total_evaluation_budget.map(|total_evaluation_budget| {
            completed_evaluation_count
                .saturating_mul(100)
                .saturating_add(current_evaluation_progress_percent.min(100))
                .checked_div(total_evaluation_budget.max(1))
                .unwrap_or(0)
                .min(99)
        })
    }

    fn record_progress_percent(&self, progress_percent: usize) {
        self.shared_progress_state
            .record_candidate_progress(self.current_restart_id, progress_percent);
    }

    fn shared_progress_snapshot(&self) -> OptionRefinementSharedProgressSnapshot {
        self.shared_progress_state.snapshot(self.total_units)
    }
}

impl OptionPricingProgressSnapshotSource for OptionRefinementProgressContext<'_> {
    fn option_pricing_progress_snapshot(
        &self,
        pricing_event_progress_percent: Option<usize>,
    ) -> OptionPricingProgressSnapshot {
        if let Some(candidate_progress_percent) =
            self.running_candidate_progress_percent(pricing_event_progress_percent)
        {
            self.record_progress_percent(candidate_progress_percent);
        }

        let shared_snapshot = self.shared_progress_snapshot();

        OptionPricingProgressSnapshot {
            completed_units: shared_snapshot.completed_units,
            aggregate_progress_percent: Some(shared_snapshot.aggregate_progress_percent),
            active_restart_count: None,
            current_evaluation_count: Some(self.current_evaluation_count()),
            total_evaluation_budget: self.total_evaluation_budget(),
            evaluation_progress_percent: Some(
                self.running_pricing_progress_percent(pricing_event_progress_percent),
            ),
            current_weighted_objective: self.current_weighted_objective(),
            best_restart_id: shared_snapshot.best_restart_id,
            best_weighted_objective: shared_snapshot.best_weighted_objective,
        }
    }
}

struct SelectionTierPricingProgressSnapshotSource<'a> {
    shared_progress_state: &'a OptionRefinementSharedProgressState,
    restart_id: &'a str,
    total_units: usize,
}

impl OptionPricingProgressSnapshotSource for SelectionTierPricingProgressSnapshotSource<'_> {
    fn option_pricing_progress_snapshot(
        &self,
        pricing_event_progress_percent: Option<usize>,
    ) -> OptionPricingProgressSnapshot {
        if let Some(pricing_event_progress_percent) = pricing_event_progress_percent {
            self.shared_progress_state
                .record_candidate_progress(self.restart_id, pricing_event_progress_percent);
        }

        let shared_snapshot = self.shared_progress_state.snapshot(self.total_units);

        OptionPricingProgressSnapshot {
            completed_units: shared_snapshot.completed_units,
            aggregate_progress_percent: Some(shared_snapshot.aggregate_progress_percent),
            active_restart_count: None,
            current_evaluation_count: None,
            total_evaluation_budget: None,
            evaluation_progress_percent: None,
            current_weighted_objective: None,
            best_restart_id: shared_snapshot.best_restart_id,
            best_weighted_objective: shared_snapshot.best_weighted_objective,
        }
    }
}

#[derive(Debug, Clone)]
struct OptionRefinementSharedProgressSnapshot {
    completed_units: usize,
    aggregate_progress_percent: usize,
    best_restart_id: Option<String>,
    best_weighted_objective: Option<f64>,
}

#[derive(Debug, Default)]
struct OptionRefinementSharedProgressState {
    completed_units: AtomicUsize,
    candidate_progress_percent_by_restart_id: Mutex<BTreeMap<String, usize>>,
    best_scored_candidate: Mutex<Option<(String, f64)>>,
    best_completed_candidate: Mutex<Option<(String, f64)>>,
}

impl OptionRefinementSharedProgressState {
    fn snapshot(&self, total_units: usize) -> OptionRefinementSharedProgressSnapshot {
        let best_candidate = self
            .candidate_score_snapshot(&self.best_completed_candidate)
            .or_else(|| self.candidate_score_snapshot(&self.best_scored_candidate));
        let (best_restart_id, best_weighted_objective) = best_candidate
            .map(|(restart_id, weighted_objective)| (Some(restart_id), Some(weighted_objective)))
            .unwrap_or((None, None));

        let aggregate_progress_percent = self.aggregate_progress_percent(total_units);

        OptionRefinementSharedProgressSnapshot {
            completed_units: self.completed_units.load(Ordering::Relaxed),
            aggregate_progress_percent,
            best_restart_id,
            best_weighted_objective,
        }
    }

    fn record_candidate_started(
        &self,
        restart_id: &str,
        total_units: usize,
    ) -> OptionRefinementSharedProgressSnapshot {
        let mut candidate_progress_guard = self
            .candidate_progress_percent_by_restart_id
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        candidate_progress_guard.insert(restart_id.to_string(), 0);
        drop(candidate_progress_guard);

        self.snapshot(total_units)
    }

    fn record_candidate_completed(
        &self,
        restart_id: &str,
        weighted_objective: Option<f64>,
        total_units: usize,
    ) -> OptionRefinementSharedProgressSnapshot {
        if let Some(weighted_objective) = weighted_objective {
            let mut best_candidate_guard = self
                .best_completed_candidate
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            let should_replace_best = best_candidate_guard
                .as_ref()
                .map(|(_, best_weighted_objective)| weighted_objective < *best_weighted_objective)
                .unwrap_or(true);
            if should_replace_best {
                *best_candidate_guard = Some((restart_id.to_string(), weighted_objective));
            }

            drop(best_candidate_guard);
        }

        let mut candidate_progress_guard = self
            .candidate_progress_percent_by_restart_id
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        candidate_progress_guard.remove(restart_id);
        drop(candidate_progress_guard);

        self.completed_units.fetch_add(1, Ordering::Relaxed);

        self.snapshot(total_units)
    }

    fn record_resumed_candidate(
        &self,
        restart_id: &str,
        weighted_objective: f64,
        total_units: usize,
    ) -> OptionRefinementSharedProgressSnapshot {
        let mut best_candidate_guard = self
            .best_completed_candidate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let should_replace_best = best_candidate_guard
            .as_ref()
            .map(|(_, best_weighted_objective)| weighted_objective < *best_weighted_objective)
            .unwrap_or(true);
        if should_replace_best {
            *best_candidate_guard = Some((restart_id.to_string(), weighted_objective));
        }

        drop(best_candidate_guard);

        self.completed_units.fetch_add(1, Ordering::Relaxed);
        self.snapshot(total_units)
    }

    fn record_candidate_progress(&self, restart_id: &str, progress_percent: usize) {
        let mut candidate_progress_guard = self
            .candidate_progress_percent_by_restart_id
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let candidate_progress = candidate_progress_guard
            .entry(restart_id.to_string())
            .or_insert(0);
        *candidate_progress = (*candidate_progress).max(progress_percent.min(100));
    }

    fn candidate_score_snapshot(
        &self,
        candidate_score: &Mutex<Option<(String, f64)>>,
    ) -> Option<(String, f64)> {
        let best_candidate_guard = candidate_score
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Snapshot payloads own restart ids so progress emission never borrows through a lock.
        best_candidate_guard
            .as_ref()
            .map(|(restart_id, weighted_objective)| (restart_id.clone(), *weighted_objective))
    }

    fn record_candidate_score(&self, restart_id: &str, weighted_objective: f64) {
        let mut best_candidate_guard = self
            .best_scored_candidate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let should_replace_best = best_candidate_guard
            .as_ref()
            .map(|(_, best_weighted_objective)| weighted_objective < *best_weighted_objective)
            .unwrap_or(true);
        if should_replace_best {
            *best_candidate_guard = Some((restart_id.to_string(), weighted_objective));
        }
    }

    fn aggregate_progress_percent(&self, total_units: usize) -> usize {
        let candidate_progress_guard = self
            .candidate_progress_percent_by_restart_id
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let active_progress_percent_sum = candidate_progress_guard.values().sum::<usize>();
        let completed_progress_percent_sum = self
            .completed_units
            .load(Ordering::Relaxed)
            .saturating_mul(100);

        completed_progress_percent_sum
            .saturating_add(active_progress_percent_sum)
            .checked_div(total_units.max(1))
            .unwrap_or(0)
            .min(100)
    }
}

#[derive(Debug, Clone, Copy)]
struct OptionSearchSelectionRankInput<'a> {
    restart_id: &'a str,
    source_rank: usize,
    search_tier_score: f64,
    selection_tier_score: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct OptionRefinedCandidate {
    pub(crate) source_rank: usize,
    pub(crate) search_tier_score: f64,
    pub(crate) selection_tier_score: f64,
    pub(crate) search_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    pub(crate) selection_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    pub(crate) restart_outcome: JointRefinementRestartOutcome,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchTierCandidate {
    pub(crate) source_rank: usize,
    pub(crate) source_dynamics_parameter_bits: Vec<u64>,
    pub(crate) search_tier_score: f64,
    pub(crate) search_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    pub(crate) restart_outcome: JointRefinementRestartOutcome,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementOptionRefinementResult {
    pub(crate) selected_restart_outcome: JointRefinementRestartOutcome,
    pub(crate) selected_search_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    pub(crate) selected_selection_tier_pricing_artifact:
        Arc<JointRefinementOptionTierPricingArtifact>,
    pub(crate) search_selection_agreement:
        Option<JointRefinementStageOptionSearchSelectionAgreementDiagnostics>,
}

pub(crate) fn desired_joint_refinement_option_refinement_candidate_count(
    candidate_count_override: Option<usize>,
) -> usize {
    candidate_count_override
        .filter(|candidate_count| *candidate_count > 0)
        .unwrap_or(JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT)
}

pub(crate) fn run_option_refinement(
    context: &JointRefinementContext,
    _solver_settings: &CalibrationOptimizerSettings,
    dynamics_candidates: &[JointRefinementRestartOutcome],
    wall_clock_budget_millis: Option<u64>,
    option_context_fingerprint: &str,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    mut resume_store: Option<&mut JointRefinementResumeStore>,
) -> Result<JointRefinementOptionRefinementResult, JointRefinementStageExecutionFailure> {
    if context.option_pricing_context.is_none() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: "joint option refinement requires an option pricing context"
                    .to_string(),
            },
        );
    }

    if dynamics_candidates.is_empty() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option refinement requires at least one coarse-consistent dynamics candidate"
                        .to_string(),
            },
        );
    }

    let search_candidates = refine_search_tier_candidates(
        context,
        dynamics_candidates,
        wall_clock_budget_millis,
        option_context_fingerprint,
        progress_sender,
        resume_store.as_deref_mut(),
    );
    if search_candidates.is_empty() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option refinement failed for every search-tier dynamics candidate"
                        .to_string(),
            },
        );
    }

    let selection_frontier = select_option_refinement_selection_frontier(search_candidates);

    let selection_candidates = evaluate_selection_tier_candidates(
        context,
        selection_frontier,
        option_context_fingerprint,
        progress_sender,
        resume_store,
    );

    let search_selection_agreement =
        build_search_selection_agreement_diagnostics(selection_candidates.as_slice());

    let Some(best_candidate) = best_selection_tier_candidate(selection_candidates) else {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option refinement failed for every selection-tier dynamics candidate"
                        .to_string(),
            },
        );
    };

    Ok(JointRefinementOptionRefinementResult {
        selected_restart_outcome: best_candidate.restart_outcome,
        selected_search_tier_pricing_artifact: best_candidate.search_tier_pricing_artifact,
        selected_selection_tier_pricing_artifact: best_candidate.selection_tier_pricing_artifact,
        search_selection_agreement,
    })
}

fn select_option_refinement_selection_frontier(
    search_candidates: Vec<SearchTierCandidate>,
) -> Vec<SearchTierCandidate> {
    let Some(best_search_tier_score) = search_candidates
        .iter()
        .map(|candidate| candidate.search_tier_score)
        .min_by(f64::total_cmp)
    else {
        return search_candidates;
    };

    let effective_sample_count = search_candidates
        .iter()
        .map(option_refinement_effective_search_sample_count)
        .max()
        .unwrap_or(1);

    let admissible_score_ceiling =
        option_refinement_selection_score_ceiling(best_search_tier_score, effective_sample_count);

    search_candidates
        .into_iter()
        .filter(|candidate| candidate.search_tier_score <= admissible_score_ceiling)
        .collect()
}

pub(super) fn option_refinement_effective_search_sample_count(
    candidate: &SearchTierCandidate,
) -> usize {
    candidate
        .search_tier_pricing_artifact
        .maturity_scores
        .iter()
        .map(|maturity_score| maturity_score.maturity_weight)
        .sum::<usize>()
        .max(1)
}

fn option_refinement_selection_score_ceiling(
    best_search_tier_score: f64,
    effective_sample_count: usize,
) -> f64 {
    best_search_tier_score + option_score_resolution(best_search_tier_score, effective_sample_count)
}

fn context_with_option_pricing_cpu_count(
    context: &JointRefinementContext,
    pricing_cpu_count: usize,
) -> JointRefinementContext {
    let mut updated_context = context.clone();
    if let Some(option_pricing_context) = updated_context.option_pricing_context.as_mut() {
        option_pricing_context.cpu_count = pricing_cpu_count.max(1);
    }

    updated_context
}

fn option_tier_pricing_interval_count(
    context: &JointRefinementContext,
    tier_kind: JointRefinementOptionSurfaceTierKind,
) -> Option<usize> {
    context
        .option_pricing_context
        .as_ref()
        .map(|option_pricing_context| {
            option_pricing_context
                .tier_pricing_inputs
                .tier(tier_kind)
                .runtime_interval_schedule
                .interval_deltas_hours
                .len()
        })
}

fn refine_search_tier_candidates(
    context: &JointRefinementContext,
    dynamics_candidates: &[JointRefinementRestartOutcome],
    wall_clock_budget_millis: Option<u64>,
    option_context_fingerprint: &str,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    mut resume_store: Option<&mut JointRefinementResumeStore>,
) -> Vec<SearchTierCandidate> {
    let total_units = dynamics_candidates.len();
    let search_interval_count =
        option_tier_pricing_interval_count(context, JointRefinementOptionSurfaceTierKind::Search);

    let shared_progress_state = Arc::new(OptionRefinementSharedProgressState::default());

    let planned_work_item_labels = planned_restart_work_item_labels(
        dynamics_candidates
            .iter()
            .map(|candidate| candidate.restart_id.as_str()),
    );

    let mut candidate_slots = vec![None; total_units];
    let mut unresolved_candidates = Vec::new();

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
        aggregate_progress_percent: Some(0),
        planned_work_item_labels: planned_work_item_labels.as_slice(),
        ..OptionRefinementProgressEmissionInput::new(
            progress_sender,
            JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            0,
            total_units,
        )
    });

    for (source_rank, dynamics_candidate) in dynamics_candidates.iter().enumerate() {
        if let Some(resume_store) = resume_store.as_deref_mut()
            && let Some(mut resumed_candidate) = resume_store.search_tier_candidate(
                option_context_fingerprint,
                dynamics_candidate.restart_id.as_str(),
            )
        {
            resumed_candidate.source_rank = source_rank;

            let resumed_search_tier_score = resumed_candidate.search_tier_score;

            let progress_snapshot = shared_progress_state.record_resumed_candidate(
                dynamics_candidate.restart_id.as_str(),
                resumed_search_tier_score,
                total_units,
            );

            emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
                current_restart_id: Some(dynamics_candidate.restart_id.as_str()),
                current_item_label: Some(dynamics_candidate.restart_id.as_str()),
                current_weighted_objective: Some(resumed_search_tier_score),
                best_restart_id: progress_snapshot.best_restart_id.as_deref(),
                best_weighted_objective: progress_snapshot.best_weighted_objective,
                active_restart_count: None,
                current_pricing_interval_index: search_interval_count,
                total_pricing_interval_count: search_interval_count,
                aggregate_progress_percent: Some(progress_snapshot.aggregate_progress_percent),
                evaluation_progress_percent: Some(100),
                ..OptionRefinementProgressEmissionInput::new(
                    progress_sender,
                    JointRefinementStageProgressPhase::OptionSearchTierRefinement,
                    progress_snapshot.completed_units,
                    total_units,
                )
            });

            candidate_slots[source_rank] = Some(resumed_candidate);

            continue;
        }

        unresolved_candidates.push((source_rank, dynamics_candidate));
    }

    let candidate_parallelism =
        option_refinement_candidate_parallelism(context, unresolved_candidates.len());

    let pricing_cpu_counts =
        option_refinement_candidate_pricing_cpu_counts(context, candidate_parallelism);

    let unresolved_candidate_count = unresolved_candidates.len().max(1);

    let wall_clock_budget =
        wall_clock_budget_millis.map(|budget_millis| OptionRefinementWallClockBudget {
            started_at: Instant::now(),
            budget_millis,
        });

    let pending_jobs = unresolved_candidates
        .into_iter()
        .zip(pricing_cpu_counts.iter().copied().cycle())
        .map(|((source_rank, dynamics_candidate), pricing_cpu_count)| {
            ParallelSearchTierCandidateRefinementInput {
                context,
                source_rank,
                dynamics_candidate,
                progress_sender,
                total_units,
                shared_progress_state: Arc::clone(&shared_progress_state),
                pricing_cpu_count,
                wall_clock_budget,
                candidate_budget_share_count: unresolved_candidate_count,
            }
        })
        .collect::<Vec<_>>();

    let mut unresolved_results = Vec::with_capacity(pending_jobs.len());
    run_static_work_pool(
        context.stage_thread_pool.as_deref(),
        StaticWorkPoolInput::new(
            &mut unresolved_results,
            pending_jobs,
            candidate_parallelism,
            refine_parallel_search_tier_candidate,
            |results: &mut Vec<(usize, Option<SearchTierCandidate>)>, result, _status| {
                results.push(result)
            },
        ),
    );

    for (source_rank, candidate) in unresolved_results {
        if let Some(candidate) = candidate {
            if let Some(resume_store) = resume_store.as_deref_mut() {
                resume_store.record_search_tier_candidate(option_context_fingerprint, &candidate);
            }

            candidate_slots[source_rank] = Some(candidate);
        }
    }

    candidate_slots.into_iter().flatten().collect()
}

fn refine_single_search_tier_candidate(
    context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementRestartOutcome,
    progress_context: Option<&OptionRefinementProgressContext<'_>>,
    pricing_cpu_count: usize,
    wall_clock_budget: Option<OptionRefinementWallClockBudget>,
    candidate_budget_share_count: usize,
) -> Result<SearchTierCandidate, String> {
    let refinement_context =
        pricing_block_refinement_context(context, dynamics_candidate, pricing_cpu_count);

    let source_dynamics_parameter_bits =
        ordered_parameter_bits(dynamics_candidate.parameter_vector.as_slice());

    let mut theta_warm_start_state = SpectralThetaWarmStartState::new();

    let option_refinement_started_at = Instant::now();

    let initial_candidate = scored_pricing_vector(
        &refinement_context,
        dynamics_candidate,
        dynamics_candidate.parameter_vector.as_slice(),
        source_dynamics_parameter_bits.as_slice(),
        JointRefinementOptionSurfaceTierKind::Search,
        true,
        "option_pricing_block_sensitivity_baseline_evaluated",
        Some(&mut theta_warm_start_state),
        progress_context,
    )?;

    let first_oracle_call_wall_clock_millis = elapsed_millis(option_refinement_started_at).max(1);

    let search_budget = option_refinement_candidate_search_budget(
        option_refinement_started_at,
        first_oracle_call_wall_clock_millis,
        wall_clock_budget,
        candidate_budget_share_count,
    );

    optimize_pricing_block_from_search_tier_sensitivities(
        &refinement_context,
        dynamics_candidate,
        initial_candidate,
        source_dynamics_parameter_bits.as_slice(),
        &mut theta_warm_start_state,
        progress_context,
        search_budget,
    )
}

pub(super) fn option_score_resolution(score: f64, effective_quote_count: usize) -> f64 {
    let effective_quote_count = effective_quote_count.max(1) as f64;

    score / (2.0 * effective_quote_count).sqrt()
}

#[derive(Debug, Clone, Copy)]
struct OptionRefinementWallClockBudget {
    started_at: Instant,
    budget_millis: u64,
}

fn option_refinement_candidate_search_budget(
    candidate_started_at: Instant,
    first_oracle_call_wall_clock_millis: u64,
    wall_clock_budget: Option<OptionRefinementWallClockBudget>,
    candidate_budget_share_count: usize,
) -> OptionRefinementCandidateSearchBudget {
    let wall_clock_guard = wall_clock_budget.map(|wall_clock_budget| {
        let elapsed_millis = elapsed_millis(wall_clock_budget.started_at);
        let remaining_millis = wall_clock_budget
            .budget_millis
            .saturating_sub(elapsed_millis);
        let candidate_share_millis = remaining_millis / candidate_budget_share_count.max(1) as u64;

        OptionRefinementCandidateWallClockGuard {
            started_at: candidate_started_at,
            budget_millis: first_oracle_call_wall_clock_millis
                .saturating_add(candidate_share_millis),
            reference_oracle_call_millis: first_oracle_call_wall_clock_millis,
        }
    });

    OptionRefinementCandidateSearchBudget {
        total_evaluation_budget: option_pricing_block_evaluation_budget(),
        wall_clock_guard,
    }
}

struct ParallelSearchTierCandidateRefinementInput<'a> {
    context: &'a JointRefinementContext,
    source_rank: usize,
    dynamics_candidate: &'a JointRefinementRestartOutcome,
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    total_units: usize,
    shared_progress_state: Arc<OptionRefinementSharedProgressState>,
    pricing_cpu_count: usize,
    wall_clock_budget: Option<OptionRefinementWallClockBudget>,
    candidate_budget_share_count: usize,
}

struct ParallelSelectionTierCandidateEvaluationInput<'a> {
    context: &'a JointRefinementContext,
    candidate_position: usize,
    search_candidate: SearchTierCandidate,
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    total_units: usize,
    shared_progress_state: Arc<OptionRefinementSharedProgressState>,
    pricing_cpu_count: usize,
}

fn refine_parallel_search_tier_candidate(
    input: ParallelSearchTierCandidateRefinementInput<'_>,
) -> (usize, Option<SearchTierCandidate>) {
    let ParallelSearchTierCandidateRefinementInput {
        context,
        source_rank,
        dynamics_candidate,
        progress_sender,
        total_units,
        shared_progress_state,
        pricing_cpu_count,
        wall_clock_budget,
        candidate_budget_share_count,
    } = input;

    let started_snapshot = shared_progress_state
        .record_candidate_started(dynamics_candidate.restart_id.as_str(), total_units);
    let search_interval_count =
        option_tier_pricing_interval_count(context, JointRefinementOptionSurfaceTierKind::Search);

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
        current_restart_id: Some(dynamics_candidate.restart_id.as_str()),
        current_item_label: Some(dynamics_candidate.restart_id.as_str()),
        best_restart_id: started_snapshot.best_restart_id.as_deref(),
        best_weighted_objective: started_snapshot.best_weighted_objective,
        active_restart_count: None,
        current_evaluation_count: Some(0),
        total_evaluation_budget: Some(option_pricing_block_evaluation_budget()),
        evaluation_progress_percent: Some(0),
        current_pricing_interval_index: Some(0),
        total_pricing_interval_count: search_interval_count,
        ..OptionRefinementProgressEmissionInput::new(
            progress_sender,
            JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            started_snapshot.completed_units,
            total_units,
        )
    });

    let progress_context = OptionRefinementProgressContext {
        progress_sender,
        phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
        total_units,
        current_restart_id: dynamics_candidate.restart_id.as_str(),
        shared_progress_state: Arc::clone(&shared_progress_state),
        current_evaluation_count: Cell::new(0),
        current_weighted_objective: Cell::new(None),
        total_evaluation_budget: Some(option_pricing_block_evaluation_budget()),
    };

    let refined_candidate = refine_single_search_tier_candidate(
        context,
        dynamics_candidate,
        Some(&progress_context),
        pricing_cpu_count,
        wall_clock_budget,
        candidate_budget_share_count,
    )
    .ok()
    .map(|candidate| SearchTierCandidate {
        source_rank,
        source_dynamics_parameter_bits: candidate.source_dynamics_parameter_bits,
        search_tier_score: candidate.search_tier_score,
        search_tier_pricing_artifact: candidate.search_tier_pricing_artifact,
        restart_outcome: candidate.restart_outcome,
    });

    let finished_snapshot = shared_progress_state.record_candidate_completed(
        dynamics_candidate.restart_id.as_str(),
        refined_candidate
            .as_ref()
            .map(|candidate| candidate.search_tier_score),
        total_units,
    );

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
        current_restart_id: Some(dynamics_candidate.restart_id.as_str()),
        current_item_label: Some(dynamics_candidate.restart_id.as_str()),
        current_weighted_objective: refined_candidate
            .as_ref()
            .map(|candidate| candidate.search_tier_score),
        best_restart_id: finished_snapshot.best_restart_id.as_deref(),
        best_weighted_objective: finished_snapshot.best_weighted_objective,
        active_restart_count: None,
        aggregate_progress_percent: Some(finished_snapshot.aggregate_progress_percent),
        current_evaluation_count: Some(progress_context.current_evaluation_count()),
        total_evaluation_budget: progress_context.total_evaluation_budget(),
        evaluation_progress_percent: Some(100),
        current_pricing_interval_index: search_interval_count,
        total_pricing_interval_count: search_interval_count,
        ..OptionRefinementProgressEmissionInput::new(
            progress_sender,
            JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            finished_snapshot.completed_units,
            total_units,
        )
    });

    (source_rank, refined_candidate)
}

fn evaluate_selection_tier_candidates(
    context: &JointRefinementContext,
    search_candidates: Vec<SearchTierCandidate>,
    option_context_fingerprint: &str,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    mut resume_store: Option<&mut JointRefinementResumeStore>,
) -> Vec<OptionRefinedCandidate> {
    let total_units = search_candidates.len();
    let selection_interval_count = option_tier_pricing_interval_count(
        context,
        JointRefinementOptionSurfaceTierKind::Selection,
    );

    let shared_progress_state = Arc::new(OptionRefinementSharedProgressState::default());
    let planned_work_item_labels = planned_restart_work_item_labels(
        search_candidates
            .iter()
            .map(|candidate| candidate.restart_outcome.restart_id.as_str()),
    );

    let mut candidate_slots = vec![None; total_units];
    let mut unresolved_candidates = Vec::new();

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
        aggregate_progress_percent: Some(0),
        planned_work_item_labels: planned_work_item_labels.as_slice(),
        ..OptionRefinementProgressEmissionInput::new(
            progress_sender,
            JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
            0,
            total_units,
        )
    });

    for (candidate_position, search_candidate) in search_candidates.into_iter().enumerate() {
        let restart_id = search_candidate.restart_outcome.restart_id.as_str();
        if let Some(resume_store) = resume_store.as_deref_mut()
            && let Some(mut resumed_candidate) =
                resume_store.selection_tier_candidate(option_context_fingerprint, restart_id)
        {
            resumed_candidate.source_rank = search_candidate.source_rank;
            let resumed_selection_tier_score = resumed_candidate.selection_tier_score;

            let progress_snapshot = shared_progress_state.record_resumed_candidate(
                restart_id,
                resumed_selection_tier_score,
                total_units,
            );

            emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
                current_restart_id: Some(restart_id),
                current_item_label: Some(restart_id),
                current_weighted_objective: Some(resumed_selection_tier_score),
                best_restart_id: progress_snapshot.best_restart_id.as_deref(),
                best_weighted_objective: progress_snapshot.best_weighted_objective,
                active_restart_count: None,
                current_pricing_interval_index: selection_interval_count,
                total_pricing_interval_count: selection_interval_count,
                aggregate_progress_percent: Some(progress_snapshot.aggregate_progress_percent),
                evaluation_progress_percent: Some(100),
                ..OptionRefinementProgressEmissionInput::new(
                    progress_sender,
                    JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
                    progress_snapshot.completed_units,
                    total_units,
                )
            });

            candidate_slots[candidate_position] = Some(resumed_candidate);

            continue;
        }

        unresolved_candidates.push((candidate_position, search_candidate));
    }

    let candidate_parallelism =
        option_refinement_candidate_parallelism(context, unresolved_candidates.len());

    let pricing_cpu_counts =
        option_refinement_candidate_pricing_cpu_counts(context, candidate_parallelism);

    let pending_jobs = unresolved_candidates
        .into_iter()
        .zip(pricing_cpu_counts.iter().copied().cycle())
        .map(
            |((candidate_position, search_candidate), pricing_cpu_count)| {
                ParallelSelectionTierCandidateEvaluationInput {
                    context,
                    candidate_position,
                    search_candidate,
                    progress_sender,
                    total_units,
                    shared_progress_state: Arc::clone(&shared_progress_state),
                    pricing_cpu_count,
                }
            },
        )
        .collect::<Vec<_>>();

    let mut unresolved_results = Vec::with_capacity(pending_jobs.len());
    run_static_work_pool(
        context.stage_thread_pool.as_deref(),
        StaticWorkPoolInput::new(
            &mut unresolved_results,
            pending_jobs,
            candidate_parallelism,
            evaluate_parallel_selection_tier_candidate,
            |results: &mut Vec<(usize, Option<OptionRefinedCandidate>)>, result, _status| {
                results.push(result)
            },
        ),
    );

    for (candidate_position, candidate) in unresolved_results {
        if let Some(candidate) = candidate {
            if let Some(resume_store) = resume_store.as_deref_mut() {
                resume_store
                    .record_selection_tier_candidate(option_context_fingerprint, &candidate);
            }
            candidate_slots[candidate_position] = Some(candidate);
        }
    }

    candidate_slots.into_iter().flatten().collect()
}

pub(super) fn scored_pricing_vector(
    refinement_context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementRestartOutcome,
    pricing_vector: &[f64],
    source_dynamics_parameter_bits: &[u64],
    tier_kind: JointRefinementOptionSurfaceTierKind,
    include_node_price_sensitivities: bool,
    stop_reason: &str,
    theta_warm_start_state: Option<&mut SpectralThetaWarmStartState>,
    progress_context: Option<&OptionRefinementProgressContext<'_>>,
) -> Result<SearchTierCandidate, String> {
    if let Some(progress_context) = progress_context {
        progress_context.record_evaluation_started();
    }

    let fitted_parameters =
        parameter_vector_to_fitted_parameters(pricing_vector, refinement_context)
            .ok_or_else(|| "pricing block parameter conversion failed".to_string())?;

    let pricing_parameter_bits = pricing_block_parameter_bits(pricing_vector, refinement_context);

    let pricing_artifact = resolve_option_surface_tier_pricing_artifact(
        OptionSurfaceTierPricingArtifactResolutionInput {
            context: refinement_context,
            fitted_parameters: &fitted_parameters,
            tier_kind,
            source_dynamics_parameter_bits,
            pricing_parameter_bits: pricing_parameter_bits.as_slice(),
            include_node_price_sensitivities,
            theta_warm_start_state,
            progress: OptionPricingProgressInput {
                progress_sender: progress_context.and_then(|context| context.progress_sender),
                progress_phase: progress_context.map(|context| context.phase),
                completed_units: progress_context
                    .map(|context| context.shared_progress_snapshot().completed_units),
                total_units: progress_context.map(|context| context.total_units),
                current_restart_id: progress_context.map(|context| context.current_restart_id),
                best_restart_id: None,
                best_weighted_objective: None,
                active_restart_count: None,
                current_evaluation_count: progress_context
                    .map(|context| context.current_evaluation_count()),
                total_evaluation_budget: progress_context
                    .and_then(|context| context.total_evaluation_budget()),
                evaluation_progress_percent: None,
                current_weighted_objective: progress_context
                    .and_then(|context| context.current_weighted_objective()),
                progress_snapshot_source: progress_context
                    .map(|context| context as &dyn OptionPricingProgressSnapshotSource),
            },
        },
    )?;

    let search_tier_score = pricing_artifact
        .score
        .root_mean_square_spread_normalized_residual;

    if let Some(progress_context) = progress_context {
        progress_context.record_weighted_objective(search_tier_score);
    }

    Ok(SearchTierCandidate {
        source_rank: 0,
        source_dynamics_parameter_bits: source_dynamics_parameter_bits.to_vec(),
        search_tier_score,
        search_tier_pricing_artifact: pricing_artifact,
        restart_outcome: restart_outcome_with_pricing_block(
            dynamics_candidate,
            pricing_vector,
            fitted_parameters,
            stop_reason,
        ),
    })
}

fn evaluate_parallel_selection_tier_candidate(
    input: ParallelSelectionTierCandidateEvaluationInput<'_>,
) -> (usize, Option<OptionRefinedCandidate>) {
    let ParallelSelectionTierCandidateEvaluationInput {
        context,
        candidate_position,
        search_candidate,
        progress_sender,
        total_units,
        shared_progress_state,
        pricing_cpu_count,
    } = input;

    let SearchTierCandidate {
        source_rank,
        source_dynamics_parameter_bits,
        search_tier_score,
        search_tier_pricing_artifact,
        restart_outcome,
    } = search_candidate;

    let restart_id = restart_outcome.restart_id.as_str();
    let started_snapshot = shared_progress_state.record_candidate_started(restart_id, total_units);
    let selection_interval_count = option_tier_pricing_interval_count(
        context,
        JointRefinementOptionSurfaceTierKind::Selection,
    );

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
        current_restart_id: Some(restart_id),
        current_item_label: Some(restart_id),
        best_restart_id: started_snapshot.best_restart_id.as_deref(),
        best_weighted_objective: started_snapshot.best_weighted_objective,
        active_restart_count: None,
        evaluation_progress_percent: Some(0),
        current_pricing_interval_index: Some(0),
        total_pricing_interval_count: selection_interval_count,
        ..OptionRefinementProgressEmissionInput::new(
            progress_sender,
            JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
            started_snapshot.completed_units,
            total_units,
        )
    });

    let selection_context = context_with_option_pricing_cpu_count(context, pricing_cpu_count);

    let pricing_parameter_bits = pricing_block_parameter_bits(
        restart_outcome.parameter_vector.as_slice(),
        &selection_context,
    );

    let best_snapshot = shared_progress_state.snapshot(total_units);
    let selection_progress_snapshot_source = SelectionTierPricingProgressSnapshotSource {
        shared_progress_state: shared_progress_state.as_ref(),
        restart_id,
        total_units,
    };

    let selection_artifact = resolve_option_surface_tier_pricing_artifact(
        OptionSurfaceTierPricingArtifactResolutionInput {
            context: &selection_context,
            fitted_parameters: &restart_outcome.final_evaluation.fitted_parameters,
            tier_kind: JointRefinementOptionSurfaceTierKind::Selection,
            source_dynamics_parameter_bits: source_dynamics_parameter_bits.as_slice(),
            pricing_parameter_bits: pricing_parameter_bits.as_slice(),
            include_node_price_sensitivities: false,
            theta_warm_start_state: None,
            progress: OptionPricingProgressInput {
                progress_sender,
                progress_phase: Some(
                    JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
                ),
                completed_units: Some(best_snapshot.completed_units),
                total_units: Some(total_units),
                current_restart_id: Some(restart_id),
                best_restart_id: best_snapshot.best_restart_id.as_deref(),
                best_weighted_objective: best_snapshot.best_weighted_objective,
                active_restart_count: None,
                current_evaluation_count: None,
                total_evaluation_budget: None,
                evaluation_progress_percent: None,
                current_weighted_objective: None,
                progress_snapshot_source: Some(&selection_progress_snapshot_source),
            },
        },
    )
    .ok();

    let selection_tier_score = selection_artifact
        .as_ref()
        .map(|selection_tier_pricing_artifact| {
            selection_tier_pricing_artifact
                .score
                .root_mean_square_spread_normalized_residual
        });

    let finished_snapshot = shared_progress_state.record_candidate_completed(
        restart_id,
        selection_tier_score,
        total_units,
    );

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
        current_restart_id: Some(restart_id),
        current_item_label: Some(restart_id),
        current_weighted_objective: selection_tier_score,
        best_restart_id: finished_snapshot.best_restart_id.as_deref(),
        best_weighted_objective: finished_snapshot.best_weighted_objective,
        active_restart_count: None,
        aggregate_progress_percent: Some(finished_snapshot.aggregate_progress_percent),
        current_evaluation_count: Some(1),
        total_evaluation_budget: Some(1),
        evaluation_progress_percent: Some(100),
        current_pricing_interval_index: selection_interval_count,
        total_pricing_interval_count: selection_interval_count,
        ..OptionRefinementProgressEmissionInput::new(
            progress_sender,
            JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
            finished_snapshot.completed_units,
            total_units,
        )
    });

    let refined_candidate = selection_artifact.map(|selection_tier_pricing_artifact| {
        let selection_tier_score = selection_tier_pricing_artifact
            .score
            .root_mean_square_spread_normalized_residual;
        OptionRefinedCandidate {
            source_rank,
            search_tier_score,
            selection_tier_score,
            search_tier_pricing_artifact,
            selection_tier_pricing_artifact,
            restart_outcome,
        }
    });

    (candidate_position, refined_candidate)
}

fn best_selection_tier_candidate(
    mut candidates: Vec<OptionRefinedCandidate>,
) -> Option<OptionRefinedCandidate> {
    candidates.sort_by(|left, right| {
        left.selection_tier_score
            .total_cmp(&right.selection_tier_score)
            .then_with(|| left.search_tier_score.total_cmp(&right.search_tier_score))
            .then_with(|| left.source_rank.cmp(&right.source_rank))
            .then_with(|| {
                left.restart_outcome
                    .restart_id
                    .cmp(&right.restart_outcome.restart_id)
            })
    });

    candidates.into_iter().next()
}

fn build_search_selection_agreement_diagnostics(
    candidates: &[OptionRefinedCandidate],
) -> Option<JointRefinementStageOptionSearchSelectionAgreementDiagnostics> {
    let rank_inputs = candidates
        .iter()
        .map(|candidate| OptionSearchSelectionRankInput {
            restart_id: candidate.restart_outcome.restart_id.as_str(),
            source_rank: candidate.source_rank,
            search_tier_score: candidate.search_tier_score,
            selection_tier_score: candidate.selection_tier_score,
        })
        .collect::<Vec<_>>();

    build_search_selection_agreement_diagnostics_from_scores(rank_inputs.as_slice())
}

fn build_search_selection_agreement_diagnostics_from_scores(
    rank_inputs: &[OptionSearchSelectionRankInput<'_>],
) -> Option<JointRefinementStageOptionSearchSelectionAgreementDiagnostics> {
    if rank_inputs.is_empty() {
        return None;
    }

    let search_ranks =
        option_candidate_ranks_by_score(rank_inputs, |rank_input| rank_input.search_tier_score);

    let selection_ranks =
        option_candidate_ranks_by_score(rank_inputs, |rank_input| rank_input.selection_tier_score);

    let best_by_search_restart_id = search_ranks
        .iter()
        .find(|(_, rank)| **rank == 1)
        .map(|(restart_id, _)| restart_id.clone());

    let best_by_selection_restart_id = selection_ranks
        .iter()
        .find(|(_, rank)| **rank == 1)
        .map(|(restart_id, _)| restart_id.clone());

    let selected_restart_id = best_by_selection_restart_id.clone();

    let selected_search_rank = selected_restart_id
        .as_ref()
        .and_then(|restart_id| search_ranks.get(restart_id))
        .copied();

    let selected_selection_rank = selected_restart_id
        .as_ref()
        .and_then(|restart_id| selection_ranks.get(restart_id))
        .copied();

    let selected_rank_movement = selected_search_rank
        .zip(selected_selection_rank)
        .map(|(search_rank, selection_rank)| selection_rank as i64 - search_rank as i64);

    let spearman_rank_correlation = (rank_inputs.len() >= 2)
        .then(|| {
            let mut search_rank_values = Vec::with_capacity(rank_inputs.len());
            let mut selection_rank_values = Vec::with_capacity(rank_inputs.len());

            for rank_input in rank_inputs {
                let restart_id = rank_input.restart_id;
                if let (Some(search_rank), Some(selection_rank)) = (
                    search_ranks.get(restart_id),
                    selection_ranks.get(restart_id),
                ) {
                    search_rank_values.push(*search_rank as f64);
                    selection_rank_values.push(*selection_rank as f64);
                }
            }

            pearson_correlation(
                search_rank_values.as_slice(),
                selection_rank_values.as_slice(),
            )
        })
        .filter(|correlation| correlation.is_finite());

    Some(
        JointRefinementStageOptionSearchSelectionAgreementDiagnostics {
            frontier_size: rank_inputs.len(),
            best_by_search_restart_id,
            best_by_selection_restart_id,
            selected_restart_id,
            selected_search_rank,
            selected_selection_rank,
            selected_rank_movement,
            spearman_rank_correlation,
        },
    )
}

fn option_candidate_ranks_by_score(
    rank_inputs: &[OptionSearchSelectionRankInput<'_>],
    score: impl Fn(&OptionSearchSelectionRankInput<'_>) -> f64,
) -> BTreeMap<String, usize> {
    let mut ranked_inputs = rank_inputs.iter().collect::<Vec<_>>();

    ranked_inputs.sort_by(|left, right| {
        score(left)
            .total_cmp(&score(right))
            .then_with(|| left.source_rank.cmp(&right.source_rank))
            .then_with(|| left.restart_id.cmp(right.restart_id))
    });

    ranked_inputs
        .into_iter()
        .enumerate()
        .map(|(rank_index, rank_input)| (rank_input.restart_id.to_string(), rank_index + 1))
        .collect()
}

fn pricing_block_refinement_context(
    context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementRestartOutcome,
    pricing_cpu_count: usize,
) -> JointRefinementContext {
    let mut refinement_context = context_with_option_pricing_cpu_count(context, pricing_cpu_count);

    refinement_context.baseline_hawkes_fitted_parameters = dynamics_candidate
        .final_evaluation
        .fitted_parameters
        .hawkes_fitted_parameters
        .clone();

    refinement_context.baseline_return_components_fitted_parameters = dynamics_candidate
        .final_evaluation
        .fitted_parameters
        .return_components_fitted_parameters
        .clone();

    refinement_context.baseline_mean_reversion_fitted_parameters = dynamics_candidate
        .final_evaluation
        .fitted_parameters
        .return_components_fitted_parameters
        .mean_reversion_fitted_parameters
        .clone();

    refinement_context.option_pricing_refinement_enabled = true;

    for parameter_index in &refinement_context
        .active_parameter_layout
        .option_pricing_hawkes_block
    {
        refinement_context.lower_bounds[*parameter_index] =
            -JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND;
        refinement_context.upper_bounds[*parameter_index] = JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND;
    }

    refinement_context
}

fn restart_outcome_with_pricing_block(
    dynamics_candidate: &JointRefinementRestartOutcome,
    pricing_vector: &[f64],
    fitted_parameters: janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    stop_reason: &str,
) -> JointRefinementRestartOutcome {
    let mut local_evaluation = dynamics_candidate.local_evaluation.clone();
    local_evaluation.fitted_parameters = fitted_parameters.clone();

    let mut final_evaluation = dynamics_candidate.final_evaluation.clone();
    final_evaluation.fitted_parameters = fitted_parameters;

    JointRefinementRestartOutcome {
        restart_id: dynamics_candidate.restart_id.clone(),
        parameter_vector: pricing_vector.to_vec(),
        local_evaluation,
        final_evaluation,
        local_evaluation_count: dynamics_candidate.local_evaluation_count,
        stop_reason: stop_reason.to_string(),
    }
}

pub(crate) fn option_pricing_block_evaluation_budget() -> usize {
    1 + 2 * pricing_block_search_coordinate_count()
}

pub(crate) fn pricing_block_search_coordinate_count() -> usize {
    option_reduced_model_parameter_count()
}

fn pricing_block_parameter_bits(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Vec<u64> {
    context
        .active_parameter_layout
        .nig_block
        .iter()
        .chain(
            context
                .active_parameter_layout
                .option_pricing_hawkes_block
                .iter(),
        )
        .map(|parameter_index| parameter_vector[*parameter_index].to_bits())
        .collect()
}

struct OptionRefinementProgressEmissionInput<'a> {
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    phase: JointRefinementStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_restart_id: Option<&'a str>,
    current_item_label: Option<&'a str>,
    current_weighted_objective: Option<f64>,
    best_restart_id: Option<&'a str>,
    best_weighted_objective: Option<f64>,
    active_restart_count: Option<usize>,
    current_evaluation_count: Option<usize>,
    total_evaluation_budget: Option<usize>,
    evaluation_progress_percent: Option<usize>,
    current_pricing_interval_index: Option<usize>,
    total_pricing_interval_count: Option<usize>,
    aggregate_progress_percent: Option<usize>,
    planned_work_item_labels: &'a [String],
}

impl<'a> OptionRefinementProgressEmissionInput<'a> {
    fn new(
        progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
        phase: JointRefinementStageProgressPhase,
        completed_units: usize,
        total_units: usize,
    ) -> Self {
        Self {
            progress_sender,
            phase,
            completed_units,
            total_units,
            current_restart_id: None,
            current_item_label: None,
            current_weighted_objective: None,
            best_restart_id: None,
            best_weighted_objective: None,
            active_restart_count: None,
            current_evaluation_count: None,
            total_evaluation_budget: None,
            evaluation_progress_percent: None,
            current_pricing_interval_index: None,
            total_pricing_interval_count: None,
            aggregate_progress_percent: None,
            planned_work_item_labels: &[],
        }
    }
}

fn emit_option_refinement_progress(input: OptionRefinementProgressEmissionInput<'_>) {
    let OptionRefinementProgressEmissionInput {
        progress_sender,
        phase,
        completed_units,
        total_units,
        current_restart_id,
        current_item_label,
        current_weighted_objective,
        best_restart_id,
        best_weighted_objective,
        active_restart_count,
        current_evaluation_count,
        total_evaluation_budget,
        evaluation_progress_percent,
        current_pricing_interval_index,
        total_pricing_interval_count,
        aggregate_progress_percent,
        planned_work_item_labels,
    } = input;

    if let Some(progress_sender) = progress_sender {
        // Progress updates own labels because they cross the observer channel.
        let _ = progress_sender.send(JointRefinementStageProgressUpdate {
            aggregate_progress_percent,
            current_restart_id: current_restart_id.map(str::to_string),
            current_item_label: current_item_label.map(str::to_string),
            active_restart_count,
            current_evaluation_count,
            total_evaluation_budget,
            evaluation_progress_percent,
            current_pricing_interval_index,
            total_pricing_interval_count,
            current_global_loss: current_weighted_objective,
            best_restart_id: best_restart_id.map(str::to_string),
            best_global_loss: best_weighted_objective,
            planned_work_item_labels: planned_work_item_labels.to_vec(),
            ..JointRefinementStageProgressUpdate::new(phase, completed_units, total_units)
        });
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Arc;
    use std::time::Instant;

    use crate::stages::joint_refinement::contracts::JointRefinementStageProgressPhase;
    use crate::stages::joint_refinement::execution::OptionPricingProgressSnapshotSource;

    use super::{
        JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT,
        OptionRefinementProgressContext, OptionRefinementSharedProgressState,
        OptionSearchSelectionRankInput, build_search_selection_agreement_diagnostics_from_scores,
        desired_joint_refinement_option_refinement_candidate_count,
        option_pricing_block_evaluation_budget, option_refinement_candidate_search_budget,
        option_refinement_selection_score_ceiling,
    };

    #[test]
    fn option_refinement_candidate_count_defaults_to_five() {
        assert_eq!(
            desired_joint_refinement_option_refinement_candidate_count(None),
            JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT,
        );
    }

    #[test]
    fn option_refinement_candidate_count_honors_positive_override() {
        assert_eq!(
            desired_joint_refinement_option_refinement_candidate_count(Some(7)),
            7,
        );
    }

    #[test]
    fn option_refinement_selection_score_ceiling_uses_spread_residual_uncertainty() {
        let best_search_tier_score = 2.0;
        let effective_sample_count = 16;

        assert_eq!(
            option_refinement_selection_score_ceiling(
                best_search_tier_score,
                effective_sample_count,
            ),
            best_search_tier_score + best_search_tier_score / 32.0f64.sqrt()
        );
    }

    #[test]
    fn option_refinement_candidate_search_budget_uses_deterministic_default() {
        let search_budget = option_refinement_candidate_search_budget(Instant::now(), 100, None, 5);

        assert_eq!(
            search_budget.total_evaluation_budget,
            option_pricing_block_evaluation_budget()
        );
        assert!(search_budget.wall_clock_guard.is_none());
    }

    #[test]
    fn search_selection_agreement_reports_single_candidate_frontier() {
        let diagnostics = build_search_selection_agreement_diagnostics_from_scores(&[rank_input(
            "baseline", 0, 2.0, 3.0,
        )])
        .expect("single-candidate frontier should produce diagnostics");

        assert_eq!(diagnostics.frontier_size, 1);
        assert_eq!(
            diagnostics.best_by_search_restart_id,
            Some("baseline".to_string())
        );
        assert_eq!(
            diagnostics.best_by_selection_restart_id,
            Some("baseline".to_string())
        );
        assert_eq!(diagnostics.selected_search_rank, Some(1));
        assert_eq!(diagnostics.selected_selection_rank, Some(1));
        assert_eq!(diagnostics.selected_rank_movement, Some(0));
        assert_eq!(diagnostics.spearman_rank_correlation, None);
    }

    #[test]
    fn search_selection_agreement_reports_matching_multiple_candidate_ranks() {
        let diagnostics = build_search_selection_agreement_diagnostics_from_scores(&[
            rank_input("baseline", 0, 1.0, 1.1),
            rank_input("halton_01", 1, 2.0, 2.1),
            rank_input("halton_02", 2, 3.0, 3.1),
        ])
        .expect("multiple-candidate frontier should produce diagnostics");

        assert_eq!(diagnostics.frontier_size, 3);
        assert_eq!(
            diagnostics.best_by_search_restart_id,
            Some("baseline".to_string())
        );
        assert_eq!(
            diagnostics.best_by_selection_restart_id,
            Some("baseline".to_string())
        );
        assert_eq!(diagnostics.selected_rank_movement, Some(0));
        assert_eq!(diagnostics.spearman_rank_correlation, Some(1.0));
    }

    #[test]
    fn search_selection_agreement_reports_rank_disagreement() {
        let diagnostics = build_search_selection_agreement_diagnostics_from_scores(&[
            rank_input("baseline", 0, 1.0, 3.0),
            rank_input("halton_01", 1, 2.0, 1.0),
            rank_input("halton_02", 2, 3.0, 2.0),
        ])
        .expect("rank-disagreement frontier should produce diagnostics");

        assert_eq!(diagnostics.frontier_size, 3);
        assert_eq!(
            diagnostics.best_by_search_restart_id,
            Some("baseline".to_string())
        );
        assert_eq!(
            diagnostics.best_by_selection_restart_id,
            Some("halton_01".to_string())
        );
        assert_eq!(
            diagnostics.selected_restart_id,
            Some("halton_01".to_string())
        );
        assert_eq!(diagnostics.selected_search_rank, Some(2));
        assert_eq!(diagnostics.selected_selection_rank, Some(1));
        assert_eq!(diagnostics.selected_rank_movement, Some(-1));
        assert_eq!(diagnostics.spearman_rank_correlation, Some(-0.5));
    }

    #[test]
    fn option_refinement_progress_context_retains_latest_completed_objective() {
        let progress_context = OptionRefinementProgressContext {
            progress_sender: None,
            phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            total_units: 5,
            current_restart_id: "baseline",
            shared_progress_state: Arc::new(OptionRefinementSharedProgressState::default()),
            current_evaluation_count: Cell::new(0),
            current_weighted_objective: Cell::new(None),
            total_evaluation_budget: None,
        };

        assert_eq!(progress_context.current_weighted_objective(), None);

        progress_context.record_weighted_objective(12.5);

        assert_eq!(progress_context.current_weighted_objective(), Some(12.5));
    }

    #[test]
    fn option_refinement_progress_context_reports_scored_objective_as_best_before_completion() {
        let shared_progress_state = Arc::new(OptionRefinementSharedProgressState::default());
        shared_progress_state.record_candidate_started("baseline", 5);

        let progress_context = OptionRefinementProgressContext {
            progress_sender: None,
            phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            total_units: 5,
            current_restart_id: "baseline",
            shared_progress_state,
            current_evaluation_count: Cell::new(0),
            current_weighted_objective: Cell::new(None),
            total_evaluation_budget: None,
        };

        progress_context.record_weighted_objective(12.5);

        let snapshot = progress_context.option_pricing_progress_snapshot(Some(100));

        assert_eq!(snapshot.best_restart_id, Some("baseline".to_string()));
        assert_eq!(snapshot.best_weighted_objective, Some(12.5));
        assert_eq!(snapshot.current_weighted_objective, Some(12.5));
    }

    #[test]
    fn option_refinement_progress_context_uses_completed_best_after_final_loss_exists() {
        let shared_progress_state = Arc::new(OptionRefinementSharedProgressState::default());
        shared_progress_state.record_candidate_completed("baseline", Some(10.0), 5);

        let progress_context = OptionRefinementProgressContext {
            progress_sender: None,
            phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            total_units: 5,
            current_restart_id: "halton_01",
            shared_progress_state,
            current_evaluation_count: Cell::new(0),
            current_weighted_objective: Cell::new(None),
            total_evaluation_budget: None,
        };

        progress_context.record_weighted_objective(4.0);

        let snapshot = progress_context.option_pricing_progress_snapshot(Some(100));

        assert_eq!(snapshot.best_restart_id, Some("baseline".to_string()));
        assert_eq!(snapshot.best_weighted_objective, Some(10.0));
    }

    fn rank_input(
        restart_id: &str,
        source_rank: usize,
        search_tier_score: f64,
        selection_tier_score: f64,
    ) -> OptionSearchSelectionRankInput<'_> {
        OptionSearchSelectionRankInput {
            restart_id,
            source_rank,
            search_tier_score,
            selection_tier_score,
        }
    }

    #[test]
    fn option_refinement_progress_context_reports_pricing_progress_inside_single_evaluation() {
        let progress_context = OptionRefinementProgressContext {
            progress_sender: None,
            phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            total_units: 5,
            current_restart_id: "baseline",
            shared_progress_state: Arc::new(OptionRefinementSharedProgressState::default()),
            current_evaluation_count: Cell::new(0),
            current_weighted_objective: Cell::new(None),
            total_evaluation_budget: Some(5),
        };

        progress_context.record_evaluation_started();

        let snapshot = progress_context.option_pricing_progress_snapshot(Some(25));

        assert_eq!(snapshot.evaluation_progress_percent, Some(5));
    }

    #[test]
    fn option_refinement_progress_context_does_not_count_unbounded_eval_in_global_progress() {
        let shared_progress_state = Arc::new(OptionRefinementSharedProgressState::default());
        shared_progress_state.record_candidate_started("baseline", 5);

        let progress_context = OptionRefinementProgressContext {
            progress_sender: None,
            phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            total_units: 5,
            current_restart_id: "baseline",
            shared_progress_state,
            current_evaluation_count: Cell::new(0),
            current_weighted_objective: Cell::new(None),
            total_evaluation_budget: None,
        };

        progress_context.record_evaluation_started();

        let snapshot = progress_context.option_pricing_progress_snapshot(Some(100));

        assert_eq!(snapshot.evaluation_progress_percent, Some(99));
        assert_eq!(snapshot.aggregate_progress_percent, Some(0));
    }

    #[test]
    fn option_refinement_shared_progress_counts_active_partial_work() {
        let progress_state = OptionRefinementSharedProgressState::default();

        progress_state.record_candidate_started("baseline", 5);
        progress_state.record_candidate_started("halton_05", 5);
        progress_state.record_candidate_progress("baseline", 25);
        progress_state.record_candidate_progress("halton_05", 50);

        let snapshot = progress_state.snapshot(5);

        assert_eq!(snapshot.completed_units, 0);
        assert_eq!(snapshot.aggregate_progress_percent, 15);
    }
}
