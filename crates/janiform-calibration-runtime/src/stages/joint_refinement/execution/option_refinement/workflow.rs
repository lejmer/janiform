use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::Instant;

use janiform_calibration_artifacts::JointRefinementStageOptionSearchSelectionAgreementDiagnostics;
use janiform_engine_pricing::internal::SpectralThetaWarmStartState;
use janiform_foundations::numeric::pearson_correlation;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate,
};
use crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTierKind;
use crate::stages::parallelism::resolved_stage_cpu_count;

use super::super::super::setup::{
    apply_option_pricing_kernel_bounds, parameter_vector_to_fitted_parameters,
};
use super::super::{
    JointRefinementCandidateOutcome, JointRefinementContext,
    JointRefinementOptionTierPricingArtifact, JointRefinementResumeStore,
    OptionPricingProgressInput, OptionPricingProgressSnapshot, OptionPricingProgressSnapshotSource,
    OptionSurfaceTierPricingArtifactResolutionInput, elapsed_millis, ordered_parameter_bits,
    resolve_option_surface_tier_pricing_artifact,
};
use super::candidate_parallelism::{
    option_refinement_candidate_parallelism, option_refinement_candidate_pricing_cpu_counts,
};
use super::candidate_tier_progress::{
    OptionCandidateTierProgressInput, OptionRefinementSharedProgressSnapshot,
    OptionRefinementSharedProgressState, ResumedOptionCandidateTierProgressInput,
    emit_completed_option_candidate_tier_progress, emit_resumed_option_candidate_tier_progress,
    emit_started_option_candidate_tier_progress, run_option_candidate_tier_work_pool,
    start_option_candidate_tier_progress,
};
use super::pricing_block_optimizer::{
    OptionRefinementCandidateSearchBudget, OptionRefinementCandidateWallClockGuard,
    optimize_pricing_block_from_search_tier_sensitivities,
};

pub(crate) const JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT: usize = 5;
pub(crate) const OPTION_PRICING_BLOCK_SEARCH_COORDINATE_COUNT: usize = 3;
const JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_SELECTION_CANDIDATE_COUNT: usize = 1;
const OPTION_SELECTION_TIER_EVALUATION_BUDGET: usize = 1;

#[derive(Debug)]
pub(super) struct OptionRefinementProgressContext<'a> {
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    phase: JointRefinementStageProgressPhase,
    total_units: usize,
    current_candidate_id: &'a str,
    shared_progress_state: Arc<OptionRefinementSharedProgressState>,
    current_evaluation_count: Cell<usize>,
    current_evaluation_progress_percent: Cell<usize>,
    current_weighted_objective: Cell<Option<f64>>,
    current_best_weighted_objective: Cell<Option<f64>>,
    total_evaluation_budget: Option<usize>,
}

impl OptionRefinementProgressContext<'_> {
    fn record_evaluation_started(&self) {
        self.current_evaluation_count
            .set(self.current_evaluation_count.get().saturating_add(1));
        self.current_evaluation_progress_percent.set(0);
    }

    pub(super) fn current_evaluation_count(&self) -> usize {
        self.current_evaluation_count.get()
    }

    fn record_weighted_objective(&self, weighted_objective: f64) {
        self.current_weighted_objective
            .set(Some(weighted_objective));

        self.current_best_weighted_objective.set(Some(
            self.current_best_weighted_objective
                .get()
                .map(|best_weighted_objective| best_weighted_objective.min(weighted_objective))
                .unwrap_or(weighted_objective),
        ));

        self.shared_progress_state
            .record_candidate_score(self.current_candidate_id, weighted_objective);
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

        let current_evaluation_progress_percent = self
            .current_evaluation_progress_percent
            .get()
            .max(pricing_event_progress_percent.unwrap_or(0).min(100));

        self.current_evaluation_progress_percent
            .set(current_evaluation_progress_percent);

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
            .record_candidate_progress(self.current_candidate_id, progress_percent);
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
            active_candidate_count: None,
            current_evaluation_count: Some(self.current_evaluation_count()),
            total_evaluation_budget: self.total_evaluation_budget(),
            evaluation_progress_percent: Some(
                self.running_pricing_progress_percent(pricing_event_progress_percent),
            ),
            current_weighted_objective: self.current_weighted_objective(),
            current_best_weighted_objective: self.current_best_weighted_objective.get(),
            best_candidate_id: shared_snapshot.best_candidate_id,
            best_weighted_objective: shared_snapshot.best_weighted_objective,
        }
    }
}

struct SelectionTierPricingProgressSnapshotSource<'a> {
    shared_progress_state: &'a OptionRefinementSharedProgressState,
    candidate_id: &'a str,
    total_units: usize,
    current_progress_percent: Cell<usize>,
}

impl OptionPricingProgressSnapshotSource for SelectionTierPricingProgressSnapshotSource<'_> {
    fn option_pricing_progress_snapshot(
        &self,
        pricing_event_progress_percent: Option<usize>,
    ) -> OptionPricingProgressSnapshot {
        let evaluation_progress_percent = pricing_event_progress_percent.map(|progress_percent| {
            let current_progress_percent = self
                .current_progress_percent
                .get()
                .max(progress_percent.min(99));

            self.current_progress_percent.set(current_progress_percent);

            current_progress_percent
        });

        if let Some(evaluation_progress_percent) = evaluation_progress_percent {
            self.shared_progress_state
                .record_candidate_current_progress(self.candidate_id, evaluation_progress_percent);
        }

        let shared_snapshot = self.shared_progress_state.snapshot(self.total_units);

        OptionPricingProgressSnapshot {
            completed_units: shared_snapshot.completed_units,
            aggregate_progress_percent: Some(shared_snapshot.aggregate_progress_percent),
            active_candidate_count: None,
            current_evaluation_count: Some(OPTION_SELECTION_TIER_EVALUATION_BUDGET),
            total_evaluation_budget: Some(OPTION_SELECTION_TIER_EVALUATION_BUDGET),
            evaluation_progress_percent,
            current_weighted_objective: None,
            current_best_weighted_objective: None,
            best_candidate_id: shared_snapshot.best_candidate_id,
            best_weighted_objective: shared_snapshot.best_weighted_objective,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct OptionSearchSelectionRankInput<'a> {
    candidate_id: &'a str,
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
    pub(crate) candidate_outcome: JointRefinementCandidateOutcome,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchTierCandidate {
    pub(crate) source_rank: usize,
    pub(crate) source_dynamics_parameter_bits: Vec<u64>,
    pub(crate) search_tier_score: f64,
    pub(crate) search_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    pub(crate) candidate_outcome: JointRefinementCandidateOutcome,
}

struct SearchTierRefinementOutput {
    candidates: Vec<SearchTierCandidate>,
    failure_messages: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementOptionRefinementResult {
    pub(crate) selected_candidate_outcome: JointRefinementCandidateOutcome,
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

fn desired_joint_refinement_option_refinement_selection_candidate_count(
    candidate_count_override: Option<usize>,
) -> usize {
    candidate_count_override
        .filter(|candidate_count| *candidate_count > 0)
        .unwrap_or(JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_SELECTION_CANDIDATE_COUNT)
}

pub(crate) fn run_option_refinement(
    context: &JointRefinementContext,
    dynamics_candidates: &[JointRefinementCandidateOutcome],
    selection_candidate_count_override: Option<usize>,
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

    let search_tier_refinement = refine_search_tier_candidates(
        context,
        dynamics_candidates,
        wall_clock_budget_millis,
        option_context_fingerprint,
        progress_sender,
        resume_store.as_deref_mut(),
    );

    let search_candidates = search_tier_refinement.candidates;

    let selection_candidate_count =
        desired_joint_refinement_option_refinement_selection_candidate_count(
            selection_candidate_count_override,
        );

    if search_candidates.is_empty() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: search_tier_refinement_failure_message(
                    search_tier_refinement.failure_messages.as_slice(),
                ),
            },
        );
    }

    let selection_frontier =
        select_option_refinement_selection_frontier(search_candidates, selection_candidate_count);

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
        selected_candidate_outcome: best_candidate.candidate_outcome,
        selected_search_tier_pricing_artifact: best_candidate.search_tier_pricing_artifact,
        selected_selection_tier_pricing_artifact: best_candidate.selection_tier_pricing_artifact,
        search_selection_agreement,
    })
}

fn select_option_refinement_selection_frontier(
    mut search_candidates: Vec<SearchTierCandidate>,
    selection_candidate_count: usize,
) -> Vec<SearchTierCandidate> {
    search_candidates.sort_by(|left, right| {
        left.search_tier_score
            .total_cmp(&right.search_tier_score)
            .then_with(|| left.source_rank.cmp(&right.source_rank))
            .then_with(|| {
                left.candidate_outcome
                    .candidate_id
                    .cmp(&right.candidate_outcome.candidate_id)
            })
    });

    search_candidates.truncate(selection_candidate_count.max(1));

    search_candidates
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

fn context_with_option_pricing_cpu_count(
    context: &JointRefinementContext,
    pricing_cpu_count: usize,
) -> JointRefinementContext {
    let mut updated_context = context.clone();
    if let Some(option_pricing_context) = updated_context.option_pricing_context.as_mut() {
        option_pricing_context.cpu_count = resolved_stage_cpu_count(pricing_cpu_count);
    }

    updated_context
}

fn refine_search_tier_candidates(
    context: &JointRefinementContext,
    dynamics_candidates: &[JointRefinementCandidateOutcome],
    wall_clock_budget_millis: Option<u64>,
    option_context_fingerprint: &str,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    mut resume_store: Option<&mut JointRefinementResumeStore>,
) -> SearchTierRefinementOutput {
    let total_units = dynamics_candidates.len();

    let shared_progress_state = start_option_candidate_tier_progress(
        progress_sender,
        JointRefinementStageProgressPhase::OptionSearchTierRefinement,
        total_units,
        dynamics_candidates
            .iter()
            .map(|candidate| candidate.candidate_id.as_str()),
    );

    let mut candidate_slots = vec![None; total_units];
    let mut unresolved_candidates = Vec::new();

    for (source_rank, dynamics_candidate) in dynamics_candidates.iter().enumerate() {
        if let Some(resume_store) = resume_store.as_deref_mut()
            && let Some(mut resumed_candidate) = resume_store.search_tier_candidate(
                option_context_fingerprint,
                dynamics_candidate.candidate_id.as_str(),
            )
        {
            resumed_candidate.source_rank = source_rank;

            let resumed_search_tier_score = resumed_candidate.search_tier_score;

            emit_resumed_option_candidate_tier_progress(ResumedOptionCandidateTierProgressInput {
                progress_sender,
                phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
                total_units,
                shared_progress_state: shared_progress_state.as_ref(),
                candidate_id: dynamics_candidate.candidate_id.as_str(),
                weighted_objective: resumed_search_tier_score,
                current_evaluation_count: None,
                total_evaluation_budget: None,
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

    let unresolved_results = run_option_candidate_tier_work_pool(
        context,
        pending_jobs,
        candidate_parallelism,
        |input, _cancellation_token| refine_parallel_search_tier_candidate(input),
    );

    let mut failure_messages = Vec::new();

    for (source_rank, candidate_result) in unresolved_results {
        match candidate_result {
            Ok(candidate) => {
                if let Some(resume_store) = resume_store.as_deref_mut() {
                    resume_store
                        .record_search_tier_candidate(option_context_fingerprint, &candidate);
                }

                candidate_slots[source_rank] = Some(candidate);
            }
            Err(error_message) => failure_messages.push(error_message),
        }
    }

    SearchTierRefinementOutput {
        candidates: candidate_slots.into_iter().flatten().collect(),
        failure_messages,
    }
}

fn search_tier_refinement_failure_message(failure_messages: &[String]) -> String {
    let mut message =
        "joint option refinement failed for every search-tier dynamics candidate".to_string();

    let Some(first_failure_message) = failure_messages.first() else {
        return message;
    };

    if failure_messages.len() == 1 {
        message.push_str("\nFirst failure: ");
        message.push_str(first_failure_message);
    } else {
        message.push_str(&format!(
            "\nFailed candidate count: {}\nFirst failure: {first_failure_message}",
            failure_messages.len(),
        ));
    }

    message
}

fn refine_single_search_tier_candidate(
    context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementCandidateOutcome,
    progress_context: Option<&OptionRefinementProgressContext<'_>>,
    pricing_cpu_count: usize,
    wall_clock_budget: Option<OptionRefinementWallClockBudget>,
    candidate_budget_share_count: usize,
) -> Result<SearchTierCandidate, String> {
    let refinement_context =
        pricing_block_refinement_context(context, dynamics_candidate, pricing_cpu_count);

    let source_dynamics_parameter_bits =
        ordered_parameter_bits(dynamics_candidate.parameter_vector.as_slice());
    let initial_pricing_vector =
        option_pricing_baseline_parameter_vector(&refinement_context, dynamics_candidate);

    let mut theta_warm_start_state = SpectralThetaWarmStartState::new();

    let option_refinement_started_at = Instant::now();

    let initial_candidate = scored_pricing_vector(ScoredPricingVectorInput {
        refinement_context: &refinement_context,
        dynamics_candidate,
        pricing_vector: initial_pricing_vector.as_slice(),
        source_dynamics_parameter_bits: source_dynamics_parameter_bits.as_slice(),
        tier_kind: JointRefinementOptionSurfaceTierKind::Search,
        include_node_price_sensitivities: true,
        stop_reason: "option_pricing_block_sensitivity_baseline_evaluated",
        theta_warm_start_state: Some(&mut theta_warm_start_state),
        progress_context,
    })?;

    let first_oracle_call_wall_clock_millis = elapsed_millis(option_refinement_started_at).max(1);

    let search_budget = option_refinement_candidate_search_budget(
        option_refinement_started_at,
        first_oracle_call_wall_clock_millis,
        wall_clock_budget,
        candidate_budget_share_count,
        pricing_block_search_coordinate_count(&refinement_context),
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

fn option_pricing_baseline_parameter_vector(
    refinement_context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementCandidateOutcome,
) -> Vec<f64> {
    let mut parameter_vector = dynamics_candidate.parameter_vector.clone();

    parameter_vector.resize(
        refinement_context.active_parameter_layout.vector_length(),
        0.0,
    );

    parameter_vector
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
    pricing_coordinate_count: usize,
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
        total_evaluation_budget: option_pricing_block_evaluation_budget(pricing_coordinate_count),
        wall_clock_guard,
    }
}

struct ParallelSearchTierCandidateRefinementInput<'a> {
    context: &'a JointRefinementContext,
    source_rank: usize,
    dynamics_candidate: &'a JointRefinementCandidateOutcome,
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
) -> (usize, Result<SearchTierCandidate, String>) {
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

    emit_started_option_candidate_tier_progress(OptionCandidateTierProgressInput {
        progress_sender,
        phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
        total_units,
        shared_progress_state: shared_progress_state.as_ref(),
        candidate_id: dynamics_candidate.candidate_id.as_str(),
        current_weighted_objective: None,
        current_evaluation_count: Some(0),
        total_evaluation_budget: Some(option_pricing_block_evaluation_budget(
            pricing_block_search_coordinate_count(context),
        )),
        evaluation_progress_percent: Some(0),
    });

    let progress_context = OptionRefinementProgressContext {
        progress_sender,
        phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
        total_units,
        current_candidate_id: dynamics_candidate.candidate_id.as_str(),
        shared_progress_state: Arc::clone(&shared_progress_state),
        current_evaluation_count: Cell::new(0),
        current_evaluation_progress_percent: Cell::new(0),
        current_weighted_objective: Cell::new(None),
        current_best_weighted_objective: Cell::new(None),
        total_evaluation_budget: Some(option_pricing_block_evaluation_budget(
            pricing_block_search_coordinate_count(context),
        )),
    };

    let refined_candidate = refine_single_search_tier_candidate(
        context,
        dynamics_candidate,
        Some(&progress_context),
        pricing_cpu_count,
        wall_clock_budget,
        candidate_budget_share_count,
    )
    .map_err(|error_message| {
        format!(
            "{}: {error_message}",
            dynamics_candidate.candidate_id.as_str()
        )
    })
    .map(|candidate| SearchTierCandidate {
        source_rank,
        source_dynamics_parameter_bits: candidate.source_dynamics_parameter_bits,
        search_tier_score: candidate.search_tier_score,
        search_tier_pricing_artifact: candidate.search_tier_pricing_artifact,
        candidate_outcome: candidate.candidate_outcome,
    });

    emit_completed_option_candidate_tier_progress(OptionCandidateTierProgressInput {
        progress_sender,
        phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
        total_units,
        shared_progress_state: shared_progress_state.as_ref(),
        candidate_id: dynamics_candidate.candidate_id.as_str(),
        current_weighted_objective: refined_candidate
            .as_ref()
            .ok()
            .map(|candidate| candidate.search_tier_score),
        current_evaluation_count: Some(progress_context.current_evaluation_count()),
        total_evaluation_budget: progress_context.total_evaluation_budget(),
        evaluation_progress_percent: Some(100),
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

    let shared_progress_state = start_option_candidate_tier_progress(
        progress_sender,
        JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
        total_units,
        search_candidates
            .iter()
            .map(|candidate| candidate.candidate_outcome.candidate_id.as_str()),
    );

    let mut candidate_slots = vec![None; total_units];
    let mut unresolved_candidates = Vec::new();

    for (candidate_position, search_candidate) in search_candidates.into_iter().enumerate() {
        let candidate_id = search_candidate.candidate_outcome.candidate_id.as_str();
        if let Some(resume_store) = resume_store.as_deref_mut()
            && let Some(mut resumed_candidate) =
                resume_store.selection_tier_candidate(option_context_fingerprint, candidate_id)
        {
            resumed_candidate.source_rank = search_candidate.source_rank;

            let resumed_selection_tier_score = resumed_candidate.selection_tier_score;

            emit_resumed_option_candidate_tier_progress(ResumedOptionCandidateTierProgressInput {
                progress_sender,
                phase: JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
                total_units,
                shared_progress_state: shared_progress_state.as_ref(),
                candidate_id,
                weighted_objective: resumed_selection_tier_score,
                current_evaluation_count: Some(OPTION_SELECTION_TIER_EVALUATION_BUDGET),
                total_evaluation_budget: Some(OPTION_SELECTION_TIER_EVALUATION_BUDGET),
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

    let unresolved_results = run_option_candidate_tier_work_pool(
        context,
        pending_jobs,
        candidate_parallelism,
        |input, _cancellation_token| evaluate_parallel_selection_tier_candidate(input),
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

pub(super) struct ScoredPricingVectorInput<'a> {
    pub(super) refinement_context: &'a JointRefinementContext,
    pub(super) dynamics_candidate: &'a JointRefinementCandidateOutcome,
    pub(super) pricing_vector: &'a [f64],
    pub(super) source_dynamics_parameter_bits: &'a [u64],
    pub(super) tier_kind: JointRefinementOptionSurfaceTierKind,
    pub(super) include_node_price_sensitivities: bool,
    pub(super) stop_reason: &'a str,
    pub(super) theta_warm_start_state: Option<&'a mut SpectralThetaWarmStartState>,
    pub(super) progress_context: Option<&'a OptionRefinementProgressContext<'a>>,
}

pub(super) fn scored_pricing_vector(
    input: ScoredPricingVectorInput<'_>,
) -> Result<SearchTierCandidate, String> {
    if let Some(progress_context) = input.progress_context {
        progress_context.record_evaluation_started();
    }

    let fitted_parameters =
        parameter_vector_to_fitted_parameters(input.pricing_vector, input.refinement_context)
            .ok_or_else(|| "pricing block parameter conversion failed".to_string())?;

    let pricing_parameter_bits =
        pricing_block_parameter_bits(input.pricing_vector, input.refinement_context);

    let pricing_artifact = resolve_option_surface_tier_pricing_artifact(
        OptionSurfaceTierPricingArtifactResolutionInput {
            context: input.refinement_context,
            fitted_parameters: &fitted_parameters,
            tier_kind: input.tier_kind,
            source_dynamics_parameter_bits: input.source_dynamics_parameter_bits,
            pricing_parameter_bits: pricing_parameter_bits.as_slice(),
            include_node_price_sensitivities: input.include_node_price_sensitivities,
            theta_warm_start_state: input.theta_warm_start_state,
            progress: OptionPricingProgressInput {
                progress_sender: input
                    .progress_context
                    .and_then(|context| context.progress_sender),
                progress_phase: input.progress_context.map(|context| context.phase),
                completed_units: input
                    .progress_context
                    .map(|context| context.shared_progress_snapshot().completed_units),
                total_units: input.progress_context.map(|context| context.total_units),
                current_candidate_id: input
                    .progress_context
                    .map(|context| context.current_candidate_id),
                best_candidate_id: None,
                best_weighted_objective: None,
                active_candidate_count: None,
                current_evaluation_count: input
                    .progress_context
                    .map(|context| context.current_evaluation_count()),
                total_evaluation_budget: input
                    .progress_context
                    .and_then(|context| context.total_evaluation_budget()),
                evaluation_progress_percent: None,
                current_weighted_objective: input
                    .progress_context
                    .and_then(|context| context.current_weighted_objective()),
                progress_snapshot_source: input
                    .progress_context
                    .map(|context| context as &dyn OptionPricingProgressSnapshotSource),
            },
        },
    )?;

    let search_tier_score = pricing_artifact
        .score
        .root_mean_square_spread_normalized_residual;

    if let Some(progress_context) = input.progress_context {
        progress_context.record_weighted_objective(search_tier_score);
    }

    Ok(SearchTierCandidate {
        source_rank: 0,
        source_dynamics_parameter_bits: input.source_dynamics_parameter_bits.to_vec(),
        search_tier_score,
        search_tier_pricing_artifact: pricing_artifact,
        candidate_outcome: candidate_outcome_with_pricing_block(
            input.dynamics_candidate,
            input.pricing_vector,
            fitted_parameters,
            input.stop_reason,
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
        candidate_outcome,
    } = search_candidate;

    let candidate_id = candidate_outcome.candidate_id.as_str();

    emit_started_option_candidate_tier_progress(OptionCandidateTierProgressInput {
        progress_sender,
        phase: JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
        total_units,
        shared_progress_state: shared_progress_state.as_ref(),
        candidate_id,
        current_weighted_objective: None,
        current_evaluation_count: Some(OPTION_SELECTION_TIER_EVALUATION_BUDGET),
        total_evaluation_budget: Some(OPTION_SELECTION_TIER_EVALUATION_BUDGET),
        evaluation_progress_percent: Some(0),
    });

    let selection_context = context_with_option_pricing_cpu_count(context, pricing_cpu_count);

    let pricing_parameter_bits = pricing_block_parameter_bits(
        candidate_outcome.parameter_vector.as_slice(),
        &selection_context,
    );

    let best_snapshot = shared_progress_state.snapshot(total_units);

    let selection_progress_snapshot_source = SelectionTierPricingProgressSnapshotSource {
        shared_progress_state: shared_progress_state.as_ref(),
        candidate_id,
        total_units,
        current_progress_percent: Cell::new(0),
    };

    let selection_artifact = resolve_option_surface_tier_pricing_artifact(
        OptionSurfaceTierPricingArtifactResolutionInput {
            context: &selection_context,
            fitted_parameters: &candidate_outcome.final_evaluation.fitted_parameters,
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
                current_candidate_id: Some(candidate_id),
                best_candidate_id: best_snapshot.best_candidate_id.as_deref(),
                best_weighted_objective: best_snapshot.best_weighted_objective,
                active_candidate_count: None,
                current_evaluation_count: Some(OPTION_SELECTION_TIER_EVALUATION_BUDGET),
                total_evaluation_budget: Some(OPTION_SELECTION_TIER_EVALUATION_BUDGET),
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

    emit_completed_option_candidate_tier_progress(OptionCandidateTierProgressInput {
        progress_sender,
        phase: JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
        total_units,
        shared_progress_state: shared_progress_state.as_ref(),
        candidate_id,
        current_weighted_objective: selection_tier_score,
        current_evaluation_count: Some(OPTION_SELECTION_TIER_EVALUATION_BUDGET),
        total_evaluation_budget: Some(OPTION_SELECTION_TIER_EVALUATION_BUDGET),
        evaluation_progress_percent: Some(100),
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
            candidate_outcome,
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
                left.candidate_outcome
                    .candidate_id
                    .cmp(&right.candidate_outcome.candidate_id)
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
            candidate_id: candidate.candidate_outcome.candidate_id.as_str(),
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

    let best_by_search_candidate_id = search_ranks
        .iter()
        .find(|(_, rank)| **rank == 1)
        .map(|(candidate_id, _)| candidate_id.clone());

    let best_by_selection_candidate_id = selection_ranks
        .iter()
        .find(|(_, rank)| **rank == 1)
        .map(|(candidate_id, _)| candidate_id.clone());

    let selected_candidate_id = best_by_selection_candidate_id.clone();

    let selected_search_rank = selected_candidate_id
        .as_ref()
        .and_then(|candidate_id| search_ranks.get(candidate_id))
        .copied();

    let selected_selection_rank = selected_candidate_id
        .as_ref()
        .and_then(|candidate_id| selection_ranks.get(candidate_id))
        .copied();

    let selected_rank_movement = selected_search_rank
        .zip(selected_selection_rank)
        .map(|(search_rank, selection_rank)| selection_rank as i64 - search_rank as i64);

    let spearman_rank_correlation = (rank_inputs.len() >= 2)
        .then(|| {
            let mut search_rank_values = Vec::with_capacity(rank_inputs.len());
            let mut selection_rank_values = Vec::with_capacity(rank_inputs.len());

            for rank_input in rank_inputs {
                let candidate_id = rank_input.candidate_id;
                if let (Some(search_rank), Some(selection_rank)) = (
                    search_ranks.get(candidate_id),
                    selection_ranks.get(candidate_id),
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
            best_by_search_candidate_id,
            best_by_selection_candidate_id,
            selected_candidate_id,
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
            .then_with(|| left.candidate_id.cmp(right.candidate_id))
    });

    ranked_inputs
        .into_iter()
        .enumerate()
        .map(|(rank_index, rank_input)| (rank_input.candidate_id.to_string(), rank_index + 1))
        .collect()
}

fn pricing_block_refinement_context(
    context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementCandidateOutcome,
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
        .mean_reversion_fitted_parameters;

    refinement_context.option_pricing_refinement_enabled = true;

    apply_option_pricing_kernel_bounds(
        &refinement_context.active_parameter_layout,
        refinement_context.lower_bounds.as_mut_slice(),
        refinement_context.upper_bounds.as_mut_slice(),
    );

    refinement_context
}

fn candidate_outcome_with_pricing_block(
    dynamics_candidate: &JointRefinementCandidateOutcome,
    pricing_vector: &[f64],
    fitted_parameters: janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    stop_reason: &str,
) -> JointRefinementCandidateOutcome {
    let mut local_evaluation = dynamics_candidate.local_evaluation.clone();
    local_evaluation.fitted_parameters = fitted_parameters.clone();

    let mut final_evaluation = dynamics_candidate.final_evaluation.clone();
    final_evaluation.fitted_parameters = fitted_parameters;

    JointRefinementCandidateOutcome {
        candidate_id: dynamics_candidate.candidate_id.clone(),
        parameter_vector: pricing_vector.to_vec(),
        local_evaluation,
        final_evaluation,
        local_evaluation_count: dynamics_candidate.local_evaluation_count,
        stop_reason: stop_reason.to_string(),
    }
}

pub(crate) fn option_pricing_block_evaluation_budget(pricing_coordinate_count: usize) -> usize {
    // One exact baseline price plus two proposal prices per active coordinate.
    // The proposal allowance matches a lower/upper coordinate sweep.
    1 + 2 * pricing_coordinate_count
}

pub(crate) fn pricing_block_search_coordinate_count(context: &JointRefinementContext) -> usize {
    context
        .active_parameter_layout
        .option_pricing_kernel_block
        .len()
}

fn pricing_block_parameter_bits(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Vec<u64> {
    context
        .active_parameter_layout
        .option_pricing_kernel_block
        .iter()
        .map(|parameter_index| parameter_vector[*parameter_index].to_bits())
        .collect()
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
        JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_SELECTION_CANDIDATE_COUNT,
        OptionRefinementProgressContext, OptionRefinementSharedProgressState,
        OptionSearchSelectionRankInput, SelectionTierPricingProgressSnapshotSource,
        build_search_selection_agreement_diagnostics_from_scores,
        desired_joint_refinement_option_refinement_candidate_count,
        desired_joint_refinement_option_refinement_selection_candidate_count,
        option_pricing_block_evaluation_budget, option_refinement_candidate_search_budget,
        search_tier_refinement_failure_message,
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
    fn option_refinement_selection_candidate_count_defaults_to_one() {
        assert_eq!(
            desired_joint_refinement_option_refinement_selection_candidate_count(None),
            JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_SELECTION_CANDIDATE_COUNT,
        );
    }

    #[test]
    fn option_refinement_selection_candidate_count_honors_positive_override() {
        assert_eq!(
            desired_joint_refinement_option_refinement_selection_candidate_count(Some(3)),
            3,
        );
    }

    #[test]
    fn option_refinement_candidate_search_budget_uses_deterministic_default() {
        let search_budget =
            option_refinement_candidate_search_budget(Instant::now(), 100, None, 5, 2);

        assert_eq!(
            search_budget.total_evaluation_budget,
            option_pricing_block_evaluation_budget(2)
        );
        assert!(search_budget.wall_clock_guard.is_none());
    }

    #[test]
    fn search_tier_failure_message_summarizes_multiple_candidate_failures() {
        assert_eq!(
            search_tier_refinement_failure_message(&[
                "candidate_a: first failure".to_string(),
                "candidate_b: second failure".to_string(),
            ]),
            "joint option refinement failed for every search-tier dynamics candidate\nFailed candidate count: 2\nFirst failure: candidate_a: first failure"
        );
    }

    #[test]
    fn search_selection_agreement_reports_single_candidate_frontier() {
        let diagnostics = build_search_selection_agreement_diagnostics_from_scores(&[rank_input(
            "baseline", 0, 2.0, 3.0,
        )])
        .expect("single-candidate frontier should produce diagnostics");

        assert_eq!(diagnostics.frontier_size, 1);
        assert_eq!(
            diagnostics.best_by_search_candidate_id,
            Some("baseline".to_string())
        );
        assert_eq!(
            diagnostics.best_by_selection_candidate_id,
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
            diagnostics.best_by_search_candidate_id,
            Some("baseline".to_string())
        );
        assert_eq!(
            diagnostics.best_by_selection_candidate_id,
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
            diagnostics.best_by_search_candidate_id,
            Some("baseline".to_string())
        );
        assert_eq!(
            diagnostics.best_by_selection_candidate_id,
            Some("halton_01".to_string())
        );
        assert_eq!(
            diagnostics.selected_candidate_id,
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
            current_candidate_id: "baseline",
            shared_progress_state: Arc::new(OptionRefinementSharedProgressState::default()),
            current_evaluation_count: Cell::new(0),
            current_evaluation_progress_percent: Cell::new(0),
            current_weighted_objective: Cell::new(None),
            current_best_weighted_objective: Cell::new(None),
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
            current_candidate_id: "baseline",
            shared_progress_state,
            current_evaluation_count: Cell::new(0),
            current_evaluation_progress_percent: Cell::new(0),
            current_weighted_objective: Cell::new(None),
            current_best_weighted_objective: Cell::new(None),
            total_evaluation_budget: None,
        };

        progress_context.record_weighted_objective(12.5);

        let snapshot = progress_context.option_pricing_progress_snapshot(Some(100));

        assert_eq!(snapshot.best_candidate_id, Some("baseline".to_string()));
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
            current_candidate_id: "halton_01",
            shared_progress_state,
            current_evaluation_count: Cell::new(0),
            current_evaluation_progress_percent: Cell::new(0),
            current_weighted_objective: Cell::new(None),
            current_best_weighted_objective: Cell::new(None),
            total_evaluation_budget: None,
        };

        progress_context.record_weighted_objective(4.0);

        let snapshot = progress_context.option_pricing_progress_snapshot(Some(100));

        assert_eq!(snapshot.best_candidate_id, Some("baseline".to_string()));
        assert_eq!(snapshot.best_weighted_objective, Some(10.0));
    }

    fn rank_input(
        candidate_id: &str,
        source_rank: usize,
        search_tier_score: f64,
        selection_tier_score: f64,
    ) -> OptionSearchSelectionRankInput<'_> {
        OptionSearchSelectionRankInput {
            candidate_id,
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
            current_candidate_id: "baseline",
            shared_progress_state: Arc::new(OptionRefinementSharedProgressState::default()),
            current_evaluation_count: Cell::new(0),
            current_evaluation_progress_percent: Cell::new(0),
            current_weighted_objective: Cell::new(None),
            current_best_weighted_objective: Cell::new(None),
            total_evaluation_budget: Some(5),
        };

        progress_context.record_evaluation_started();

        let snapshot = progress_context.option_pricing_progress_snapshot(Some(25));

        assert_eq!(snapshot.evaluation_progress_percent, Some(5));
    }

    #[test]
    fn option_refinement_progress_context_keeps_evaluation_progress_monotone() {
        let progress_context = OptionRefinementProgressContext {
            progress_sender: None,
            phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            total_units: 5,
            current_candidate_id: "baseline",
            shared_progress_state: Arc::new(OptionRefinementSharedProgressState::default()),
            current_evaluation_count: Cell::new(0),
            current_evaluation_progress_percent: Cell::new(0),
            current_weighted_objective: Cell::new(None),
            current_best_weighted_objective: Cell::new(None),
            total_evaluation_budget: Some(5),
        };

        progress_context.record_evaluation_started();

        let advanced_snapshot = progress_context.option_pricing_progress_snapshot(Some(80));
        let candidateed_internal_pass_snapshot =
            progress_context.option_pricing_progress_snapshot(Some(10));

        assert_eq!(
            advanced_snapshot.evaluation_progress_percent,
            candidateed_internal_pass_snapshot.evaluation_progress_percent
        );
    }

    #[test]
    fn option_refinement_progress_context_does_not_count_unbounded_eval_in_global_progress() {
        let shared_progress_state = Arc::new(OptionRefinementSharedProgressState::default());
        shared_progress_state.record_candidate_started("baseline", 5);

        let progress_context = OptionRefinementProgressContext {
            progress_sender: None,
            phase: JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            total_units: 5,
            current_candidate_id: "baseline",
            shared_progress_state,
            current_evaluation_count: Cell::new(0),
            current_evaluation_progress_percent: Cell::new(0),
            current_weighted_objective: Cell::new(None),
            current_best_weighted_objective: Cell::new(None),
            total_evaluation_budget: None,
        };

        progress_context.record_evaluation_started();

        let snapshot = progress_context.option_pricing_progress_snapshot(Some(100));

        assert_eq!(snapshot.evaluation_progress_percent, Some(99));
        assert_eq!(snapshot.aggregate_progress_percent, Some(0));
    }

    #[test]
    fn selection_tier_progress_reports_single_eval_and_current_pricing_progress() {
        let shared_progress_state = OptionRefinementSharedProgressState::default();
        shared_progress_state.record_candidate_started("baseline", 1);

        let progress_source = SelectionTierPricingProgressSnapshotSource {
            shared_progress_state: &shared_progress_state,
            candidate_id: "baseline",
            total_units: 1,
            current_progress_percent: Cell::new(0),
        };

        let first_snapshot = progress_source.option_pricing_progress_snapshot(Some(100));

        assert_eq!(first_snapshot.current_evaluation_count, Some(1));
        assert_eq!(first_snapshot.total_evaluation_budget, Some(1));
        assert_eq!(first_snapshot.evaluation_progress_percent, Some(99));
        assert_eq!(first_snapshot.aggregate_progress_percent, Some(99));

        let second_snapshot = progress_source.option_pricing_progress_snapshot(Some(25));

        assert_eq!(second_snapshot.current_evaluation_count, Some(1));
        assert_eq!(second_snapshot.total_evaluation_budget, Some(1));
        assert_eq!(second_snapshot.evaluation_progress_percent, Some(99));
        assert_eq!(second_snapshot.aggregate_progress_percent, Some(99));
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
