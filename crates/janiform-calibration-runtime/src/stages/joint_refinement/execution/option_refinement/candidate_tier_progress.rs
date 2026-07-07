use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use janiform_foundations::cancellation::CancellationToken;
use janiform_work_scheduler::{StaticWorkPoolInput, run_static_work_pool};

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};

use super::super::{JointRefinementContext, planned_candidate_work_item_labels};

#[derive(Debug, Clone)]
pub(super) struct OptionRefinementSharedProgressSnapshot {
    pub(super) completed_units: usize,
    pub(super) aggregate_progress_percent: usize,
    pub(super) best_candidate_id: Option<String>,
    pub(super) best_weighted_objective: Option<f64>,
}

#[derive(Debug, Default)]
pub(super) struct OptionRefinementSharedProgressState {
    completed_units: AtomicUsize,
    candidate_progress_percent_by_candidate_id: Mutex<BTreeMap<String, usize>>,
    best_scored_candidate: Mutex<Option<(String, f64)>>,
    best_completed_candidate: Mutex<Option<(String, f64)>>,
}

impl OptionRefinementSharedProgressState {
    pub(super) fn snapshot(&self, total_units: usize) -> OptionRefinementSharedProgressSnapshot {
        let best_candidate = self
            .candidate_score_snapshot(&self.best_completed_candidate)
            .or_else(|| self.candidate_score_snapshot(&self.best_scored_candidate));

        let (best_candidate_id, best_weighted_objective) = best_candidate
            .map(|(candidate_id, weighted_objective)| {
                (Some(candidate_id), Some(weighted_objective))
            })
            .unwrap_or((None, None));

        let aggregate_progress_percent = self.aggregate_progress_percent(total_units);

        OptionRefinementSharedProgressSnapshot {
            completed_units: self.completed_units.load(Ordering::Relaxed),
            aggregate_progress_percent,
            best_candidate_id,
            best_weighted_objective,
        }
    }

    pub(super) fn record_candidate_started(
        &self,
        candidate_id: &str,
        total_units: usize,
    ) -> OptionRefinementSharedProgressSnapshot {
        let mut candidate_progress_guard = self
            .candidate_progress_percent_by_candidate_id
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        candidate_progress_guard.insert(candidate_id.to_string(), 0);

        drop(candidate_progress_guard);

        self.snapshot(total_units)
    }

    pub(super) fn record_candidate_completed(
        &self,
        candidate_id: &str,
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
                *best_candidate_guard = Some((candidate_id.to_string(), weighted_objective));
            }

            drop(best_candidate_guard);
        }

        let mut candidate_progress_guard = self
            .candidate_progress_percent_by_candidate_id
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        candidate_progress_guard.remove(candidate_id);

        drop(candidate_progress_guard);

        self.completed_units.fetch_add(1, Ordering::Relaxed);

        self.snapshot(total_units)
    }

    pub(super) fn record_resumed_candidate(
        &self,
        candidate_id: &str,
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
            *best_candidate_guard = Some((candidate_id.to_string(), weighted_objective));
        }

        drop(best_candidate_guard);

        self.completed_units.fetch_add(1, Ordering::Relaxed);
        self.snapshot(total_units)
    }

    pub(super) fn record_candidate_progress(&self, candidate_id: &str, progress_percent: usize) {
        self.update_candidate_progress(candidate_id, |candidate_progress| {
            *candidate_progress = (*candidate_progress).max(progress_percent.min(100));
        });
    }

    pub(super) fn record_candidate_current_progress(
        &self,
        candidate_id: &str,
        progress_percent: usize,
    ) {
        self.update_candidate_progress(candidate_id, |candidate_progress| {
            *candidate_progress = progress_percent.min(100);
        });
    }

    fn update_candidate_progress(
        &self,
        candidate_id: &str,
        update_progress: impl FnOnce(&mut usize),
    ) {
        let mut candidate_progress_guard = self
            .candidate_progress_percent_by_candidate_id
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let candidate_progress = candidate_progress_guard
            .entry(candidate_id.to_string())
            .or_insert(0);

        update_progress(candidate_progress);
    }

    fn candidate_score_snapshot(
        &self,
        candidate_score: &Mutex<Option<(String, f64)>>,
    ) -> Option<(String, f64)> {
        let best_candidate_guard = candidate_score
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Snapshot payloads own candidate ids so progress emission never borrows through a lock.
        best_candidate_guard
            .as_ref()
            .map(|(candidate_id, weighted_objective)| (candidate_id.clone(), *weighted_objective))
    }

    pub(super) fn record_candidate_score(&self, candidate_id: &str, weighted_objective: f64) {
        let mut best_candidate_guard = self
            .best_scored_candidate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let should_replace_best = best_candidate_guard
            .as_ref()
            .map(|(_, best_weighted_objective)| weighted_objective < *best_weighted_objective)
            .unwrap_or(true);
        if should_replace_best {
            *best_candidate_guard = Some((candidate_id.to_string(), weighted_objective));
        }
    }

    fn aggregate_progress_percent(&self, total_units: usize) -> usize {
        let candidate_progress_guard = self
            .candidate_progress_percent_by_candidate_id
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

pub(super) struct OptionCandidateTierProgressInput<'a> {
    pub(super) progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    pub(super) phase: JointRefinementStageProgressPhase,
    pub(super) total_units: usize,
    pub(super) shared_progress_state: &'a OptionRefinementSharedProgressState,
    pub(super) candidate_id: &'a str,
    pub(super) current_weighted_objective: Option<f64>,
    pub(super) current_evaluation_count: Option<usize>,
    pub(super) total_evaluation_budget: Option<usize>,
    pub(super) evaluation_progress_percent: Option<usize>,
}

pub(super) struct ResumedOptionCandidateTierProgressInput<'a> {
    pub(super) progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    pub(super) phase: JointRefinementStageProgressPhase,
    pub(super) total_units: usize,
    pub(super) shared_progress_state: &'a OptionRefinementSharedProgressState,
    pub(super) candidate_id: &'a str,
    pub(super) weighted_objective: f64,
    pub(super) current_evaluation_count: Option<usize>,
    pub(super) total_evaluation_budget: Option<usize>,
}

pub(super) fn start_option_candidate_tier_progress<'a, I>(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    phase: JointRefinementStageProgressPhase,
    total_units: usize,
    candidate_ids: I,
) -> Arc<OptionRefinementSharedProgressState>
where
    I: IntoIterator<Item = &'a str>,
{
    let shared_progress_state = Arc::new(OptionRefinementSharedProgressState::default());
    let planned_work_item_labels = planned_candidate_work_item_labels(candidate_ids);

    emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
        aggregate_progress_percent: Some(0),
        planned_work_item_labels: planned_work_item_labels.as_slice(),
        ..OptionRefinementProgressEmissionInput::new(progress_sender, phase, 0, total_units)
    });

    shared_progress_state
}

pub(super) fn emit_resumed_option_candidate_tier_progress(
    input: ResumedOptionCandidateTierProgressInput<'_>,
) -> OptionRefinementSharedProgressSnapshot {
    let progress_snapshot = input.shared_progress_state.record_resumed_candidate(
        input.candidate_id,
        input.weighted_objective,
        input.total_units,
    );

    emit_option_candidate_tier_progress(
        OptionCandidateTierProgressInput {
            progress_sender: input.progress_sender,
            phase: input.phase,
            total_units: input.total_units,
            shared_progress_state: input.shared_progress_state,
            candidate_id: input.candidate_id,
            current_weighted_objective: Some(input.weighted_objective),
            current_evaluation_count: input.current_evaluation_count,
            total_evaluation_budget: input.total_evaluation_budget,
            evaluation_progress_percent: Some(100),
        },
        &progress_snapshot,
    );

    progress_snapshot
}

pub(super) fn emit_started_option_candidate_tier_progress(
    input: OptionCandidateTierProgressInput<'_>,
) -> OptionRefinementSharedProgressSnapshot {
    let progress_snapshot = input
        .shared_progress_state
        .record_candidate_started(input.candidate_id, input.total_units);

    emit_option_candidate_tier_progress(input, &progress_snapshot);

    progress_snapshot
}

pub(super) fn emit_completed_option_candidate_tier_progress(
    input: OptionCandidateTierProgressInput<'_>,
) -> OptionRefinementSharedProgressSnapshot {
    let progress_snapshot = input.shared_progress_state.record_candidate_completed(
        input.candidate_id,
        input.current_weighted_objective,
        input.total_units,
    );

    emit_option_candidate_tier_progress(input, &progress_snapshot);

    progress_snapshot
}

fn emit_option_candidate_tier_progress(
    input: OptionCandidateTierProgressInput<'_>,
    progress_snapshot: &OptionRefinementSharedProgressSnapshot,
) {
    emit_option_refinement_progress(OptionRefinementProgressEmissionInput {
        current_candidate_id: Some(input.candidate_id),
        current_item_label: Some(input.candidate_id),
        current_weighted_objective: input.current_weighted_objective,
        best_candidate_id: progress_snapshot.best_candidate_id.as_deref(),
        best_weighted_objective: progress_snapshot.best_weighted_objective,
        active_candidate_count: None,
        aggregate_progress_percent: Some(progress_snapshot.aggregate_progress_percent),
        current_evaluation_count: input.current_evaluation_count,
        total_evaluation_budget: input.total_evaluation_budget,
        evaluation_progress_percent: input.evaluation_progress_percent,
        ..OptionRefinementProgressEmissionInput::new(
            input.progress_sender,
            input.phase,
            progress_snapshot.completed_units,
            input.total_units,
        )
    });
}

pub(super) fn run_option_candidate_tier_work_pool<'scope, Job, Output, Execute>(
    context: &'scope JointRefinementContext,
    pending_jobs: Vec<Job>,
    candidate_parallelism: usize,
    execute_job: Execute,
) -> Vec<Output>
where
    Job: Send + 'scope,
    Output: Send + 'scope,
    Execute: Fn(Job, CancellationToken) -> Output + Send + Sync + 'scope,
{
    let mut unresolved_results = Vec::with_capacity(pending_jobs.len());

    run_static_work_pool(
        context.stage_thread_pool.as_deref(),
        StaticWorkPoolInput::new(
            &mut unresolved_results,
            context.cancellation_token.shared_handle(),
            pending_jobs,
            candidate_parallelism,
            execute_job,
            |results: &mut Vec<Output>, result, _status| results.push(result),
        ),
    );

    unresolved_results
}

struct OptionRefinementProgressEmissionInput<'a> {
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    phase: JointRefinementStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_candidate_id: Option<&'a str>,
    current_item_label: Option<&'a str>,
    current_weighted_objective: Option<f64>,
    best_candidate_id: Option<&'a str>,
    best_weighted_objective: Option<f64>,
    active_candidate_count: Option<usize>,
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
            current_candidate_id: None,
            current_item_label: None,
            current_weighted_objective: None,
            best_candidate_id: None,
            best_weighted_objective: None,
            active_candidate_count: None,
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
        current_candidate_id,
        current_item_label,
        current_weighted_objective,
        best_candidate_id,
        best_weighted_objective,
        active_candidate_count,
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
            current_candidate_id: current_candidate_id.map(str::to_string),
            current_item_label: current_item_label.map(str::to_string),
            active_candidate_count,
            current_evaluation_count,
            total_evaluation_budget,
            evaluation_progress_percent,
            current_pricing_interval_index,
            total_pricing_interval_count,
            current_global_loss: current_weighted_objective,
            best_candidate_id: best_candidate_id.map(str::to_string),
            best_global_loss: best_weighted_objective,
            planned_work_item_labels: planned_work_item_labels.to_vec(),
            ..JointRefinementStageProgressUpdate::new(phase, completed_units, total_units)
        });
    }
}
