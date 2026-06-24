use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use super::super::super::{JointRefinementContext, ordered_parameter_bits};
use super::super::workflow::{
    JointRefinementBudgetSchedule, population_control_reference_evaluation_budget,
};
use super::archive::{
    best_population_control_score_record, best_restart_population_control_archive_record,
    distinct_ranked_population_control_archive_record, ranked_population_control_archive_records,
};
use super::policy::population_control_elite_count;
use super::sampling::{
    PopulationControlDistribution, PopulationControlGenerationSamplingInput,
    initial_population_control_distribution, population_control_distribution_from_elites,
    sample_population_control_generation,
};
use super::scoring::PopulationControlScore;
use super::types::{
    PopulationControlCandidateSpec, PopulationControlFinalizationJob, PopulationControlFitSnapshot,
    PopulationControlSearchJob, PopulationControlSearchResult,
};
use super::{
    POPULATION_CONTROL_SCHEDULE_CATEGORY_SAMPLE, POPULATION_CONTROL_SCHEDULE_CATEGORY_SEED,
    POPULATION_CONTROL_STOP_REASON_NO_SCHEDULABLE_WORK, PopulationControlCandidateRecord,
};

#[derive(Debug)]
pub(super) struct PopulationControlState {
    restart_ids: Vec<String>,
    seed_vectors: Vec<Vec<f64>>,
    distribution: Option<PopulationControlDistribution>,
    scheduled_parameter_bits: Vec<Vec<u64>>,
    archive: Vec<PopulationControlCandidateRecord>,
    latest_score_snapshot: Option<PopulationControlFitSnapshot>,
    best_score_snapshot: Option<PopulationControlFitSnapshot>,
    best_global_snapshot: Option<PopulationControlFitSnapshot>,
    restart_scheduled_search_counts: Vec<usize>,
    restart_completed_search_counts: Vec<usize>,
    max_search_count_per_restart: usize,
    scheduled_search_evaluation_count: usize,
    completed_search_evaluation_count: usize,
    in_flight_search_count: usize,
    scheduled_finalization_count: usize,
    completed_finalization_count: usize,
    seed_candidates_prepared: bool,
    next_generation_index: usize,
    next_restart_position: usize,
    search_closed: bool,
    stop_reason: String,
}

pub(super) fn build_population_control_state(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
) -> PopulationControlState {
    let reference_budget =
        population_control_reference_evaluation_budget(context, solver_settings).max(1);

    let max_search_count_per_restart = ((reference_budget as f64)
        * budget_schedule
            .calibration_effort
            .restart_pool_max_candidate_budget_scale)
        .ceil()
        .max(1.0) as usize;

    let restart_ids = restart_seeds
        .iter()
        // Population state owns restart labels because restart seeds are borrowed
        // from workflow input.
        .map(|(restart_id, _)| restart_id.clone())
        .collect::<Vec<_>>();

    let seed_vectors = restart_seeds
        .iter()
        // Population state owns seed vectors because worker jobs borrow state
        // across independent scheduling passes.
        .map(|(_, seed_vector)| seed_vector.clone())
        .collect::<Vec<_>>();

    let distribution = initial_population_control_distribution(
        seed_vectors.as_slice(),
        context.lower_bounds.as_slice(),
        context.upper_bounds.as_slice(),
    );

    PopulationControlState {
        restart_scheduled_search_counts: vec![0; restart_seeds.len()],
        restart_completed_search_counts: vec![0; restart_seeds.len()],
        restart_ids,
        seed_vectors,
        distribution,
        scheduled_parameter_bits: Vec::new(),
        archive: Vec::new(),
        latest_score_snapshot: None,
        best_score_snapshot: None,
        best_global_snapshot: None,
        max_search_count_per_restart,
        scheduled_search_evaluation_count: 0,
        completed_search_evaluation_count: 0,
        in_flight_search_count: 0,
        scheduled_finalization_count: 0,
        completed_finalization_count: 0,
        seed_candidates_prepared: false,
        next_generation_index: 0,
        next_restart_position: 0,
        search_closed: false,
        stop_reason: POPULATION_CONTROL_STOP_REASON_NO_SCHEDULABLE_WORK.to_string(),
    }
}

pub(super) fn record_population_control_search_result(
    state: &mut PopulationControlState,
    result: PopulationControlSearchResult,
    _solver_settings: &CalibrationOptimizerSettings,
) {
    state.in_flight_search_count = state.in_flight_search_count.saturating_sub(1);

    state.completed_search_evaluation_count =
        state.completed_search_evaluation_count.saturating_add(1);

    if let Some(completed_count) = state
        .restart_completed_search_counts
        .get_mut(result.restart_position)
    {
        *completed_count = completed_count.saturating_add(1);
    }

    let Some(evaluation) = result.evaluation else {
        return;
    };

    let score = PopulationControlScore::from_evaluation(&evaluation);
    let exact_likelihood_evaluator_counts = evaluation.exact_likelihood_evaluator_counts;

    state.update_score_snapshots(
        result.restart_id.as_str(),
        &score,
        exact_likelihood_evaluator_counts,
    );

    state.archive.push(PopulationControlCandidateRecord {
        restart_id: result.restart_id,
        parameter_vector: result.parameter_vector,
        candidate_evaluation_count: result.candidate_evaluation_count,
        score,
    });
}

impl PopulationControlState {
    pub(super) fn restart_count(&self) -> usize {
        self.restart_ids.len()
    }

    pub(super) fn scheduled_search_evaluation_count(&self) -> usize {
        self.scheduled_search_evaluation_count
    }

    pub(super) fn completed_search_evaluation_count(&self) -> usize {
        self.completed_search_evaluation_count
    }

    pub(super) fn scheduled_finalization_count(&self) -> usize {
        self.scheduled_finalization_count
    }

    pub(super) fn completed_finalization_count(&self) -> usize {
        self.completed_finalization_count
    }

    pub(super) fn in_flight_search_count(&self) -> usize {
        self.in_flight_search_count
    }

    pub(super) fn search_closed(&self) -> bool {
        self.search_closed
    }

    pub(super) fn latest_score_snapshot(&self) -> Option<&PopulationControlFitSnapshot> {
        self.latest_score_snapshot.as_ref()
    }

    pub(super) fn best_score_snapshot(&self) -> Option<&PopulationControlFitSnapshot> {
        self.best_score_snapshot.as_ref()
    }

    pub(super) fn best_global_snapshot(&self) -> Option<&PopulationControlFitSnapshot> {
        self.best_global_snapshot.as_ref()
    }

    pub(super) fn restart_search_idle(&self, restart_position: usize) -> bool {
        self.restart_scheduled_search_counts
            .get(restart_position)
            .copied()
            .unwrap_or(0)
            == self
                .restart_completed_search_counts
                .get(restart_position)
                .copied()
                .unwrap_or(0)
    }

    pub(super) fn restart_search_budget_exhausted(&self, restart_position: usize) -> bool {
        self.restart_scheduled_search_counts
            .get(restart_position)
            .copied()
            .unwrap_or(0)
            >= self.max_search_count_per_restart
    }

    pub(super) fn mark_search_closed(&mut self, stop_reason: &str) {
        self.search_closed = true;
        self.stop_reason = stop_reason.to_string();
    }

    pub(super) fn record_finalization_scheduled(&mut self) {
        self.scheduled_finalization_count = self.scheduled_finalization_count.saturating_add(1);
    }

    pub(super) fn record_finalization_completed(&mut self) {
        self.completed_finalization_count = self.completed_finalization_count.saturating_add(1);
    }

    pub(super) fn finalization_job_for_restart(
        &self,
        restart_position: usize,
    ) -> Option<PopulationControlFinalizationJob> {
        let restart_id = self.restart_ids.get(restart_position)?;
        let seed_vector = self.seed_vectors.get(restart_position)?;

        let candidate_vector =
            self.finalization_candidate_vector(restart_position, self.search_closed)?;

        let evaluation_count = self
            .restart_completed_search_counts
            .get(restart_position)
            .copied()
            .unwrap_or(0);

        // Finalization jobs own vectors and labels because seeded validation runs
        // outside the search-state borrow.
        Some(PopulationControlFinalizationJob {
            restart_position,
            restart_id: restart_id.clone(),
            seed_vector: seed_vector.clone(),
            candidate_vector,
            evaluation_count,
            stop_reason: self.stop_reason.clone(),
        })
    }

    pub(super) fn prepare_population_candidates(
        &mut self,
        context: &JointRefinementContext,
        _solver_settings: &CalibrationOptimizerSettings,
        generation_width: usize,
        remaining_search_budget: usize,
    ) -> Vec<PopulationControlCandidateSpec> {
        if !self.seed_candidates_prepared {
            return self.prepare_seed_candidates(remaining_search_budget);
        }

        self.update_distribution_from_archive(
            population_control_elite_count(generation_width),
            context.lower_bounds.as_slice(),
            context.upper_bounds.as_slice(),
        );

        let Some(distribution) = self.distribution.as_ref() else {
            return Vec::new();
        };

        let generation_index = self.next_generation_index;

        self.next_generation_index = self.next_generation_index.saturating_add(1);

        let sampled_vectors =
            sample_population_control_generation(PopulationControlGenerationSamplingInput {
                distribution,
                lower_bounds: context.lower_bounds.as_slice(),
                upper_bounds: context.upper_bounds.as_slice(),
                root_seed: context.local_simulation_seed,
                generation_index,
                generation_width: generation_width.min(remaining_search_budget),
                scheduled_parameter_bits: self.scheduled_parameter_bits.as_slice(),
            });

        self.population_candidate_specs(
            sampled_vectors,
            POPULATION_CONTROL_SCHEDULE_CATEGORY_SAMPLE,
            Some,
            "population_candidate",
            remaining_search_budget,
        )
    }

    fn prepare_seed_candidates(
        &mut self,
        remaining_search_budget: usize,
    ) -> Vec<PopulationControlCandidateSpec> {
        self.seed_candidates_prepared = true;
        self.next_generation_index = self.next_generation_index.saturating_add(1);

        // Seed candidates own vectors because queued work may outlive this
        // scheduling pass.
        let seed_vectors = self.seed_vectors.clone();

        self.population_candidate_specs(
            seed_vectors,
            POPULATION_CONTROL_SCHEDULE_CATEGORY_SEED,
            |_| None,
            "population_seed",
            remaining_search_budget,
        )
    }

    fn population_candidate_specs<F>(
        &mut self,
        candidate_vectors: Vec<Vec<f64>>,
        schedule_category: &'static str,
        search_direction_position: F,
        search_direction_label: &'static str,
        remaining_search_budget: usize,
    ) -> Vec<PopulationControlCandidateSpec>
    where
        F: Fn(usize) -> Option<usize>,
    {
        let mut candidate_specs = Vec::new();

        // Candidate preparation tracks restart reservations locally so the queue
        // does not fill with work that the per-restart budget will reject.
        let mut prepared_restart_counts = self.restart_scheduled_search_counts.clone();

        for (candidate_position, parameter_vector) in candidate_vectors.into_iter().enumerate() {
            if candidate_specs.len() >= remaining_search_budget {
                break;
            }

            let parameter_bits = ordered_parameter_bits(parameter_vector.as_slice());
            if self
                .scheduled_parameter_bits
                .iter()
                .any(|scheduled_bits| scheduled_bits == &parameter_bits)
            {
                continue;
            }

            let Some(restart_position) =
                self.next_restart_position_with_prepared_budget(&mut prepared_restart_counts)
            else {
                break;
            };

            self.scheduled_parameter_bits.push(parameter_bits);

            let Some(restart_id) = self.restart_ids.get(restart_position) else {
                continue;
            };

            // Candidate specs own restart labels and vectors so worker scheduling
            // does not borrow mutable population state.
            candidate_specs.push(PopulationControlCandidateSpec {
                restart_position,
                restart_id: restart_id.clone(),
                parameter_vector,
                schedule_category,
                search_direction_position: search_direction_position(candidate_position),
                search_direction_label: search_direction_label.to_string(),
            });
        }

        candidate_specs
    }

    pub(super) fn schedule_search_job(
        &mut self,
        candidate_spec: PopulationControlCandidateSpec,
    ) -> Option<PopulationControlSearchJob> {
        let scheduled_count = self
            .restart_scheduled_search_counts
            .get_mut(candidate_spec.restart_position)?;

        if *scheduled_count >= self.max_search_count_per_restart {
            return None;
        }

        *scheduled_count = scheduled_count.saturating_add(1);

        self.scheduled_search_evaluation_count =
            self.scheduled_search_evaluation_count.saturating_add(1);

        self.in_flight_search_count = self.in_flight_search_count.saturating_add(1);

        Some(PopulationControlSearchJob {
            restart_position: candidate_spec.restart_position,
            restart_id: candidate_spec.restart_id,
            parameter_vector: candidate_spec.parameter_vector,
            candidate_evaluation_count: *scheduled_count,
            schedule_category: candidate_spec.schedule_category,
            search_direction_position: candidate_spec.search_direction_position,
            search_direction_label: candidate_spec.search_direction_label,
        })
    }

    fn next_restart_position_with_prepared_budget(
        &mut self,
        prepared_restart_counts: &mut [usize],
    ) -> Option<usize> {
        if self.restart_ids.is_empty() {
            return None;
        }

        for _ in 0..self.restart_ids.len() {
            let restart_position = self.next_restart_position % self.restart_ids.len();

            self.next_restart_position =
                self.next_restart_position.saturating_add(1) % self.restart_ids.len();

            let Some(prepared_restart_count) = prepared_restart_counts.get_mut(restart_position)
            else {
                continue;
            };

            if *prepared_restart_count < self.max_search_count_per_restart {
                *prepared_restart_count = prepared_restart_count.saturating_add(1);

                return Some(restart_position);
            }
        }

        None
    }

    fn update_distribution_from_archive(
        &mut self,
        elite_count: usize,
        lower_bounds: &[f64],
        upper_bounds: &[f64],
    ) {
        let Some(previous_distribution) = self.distribution.as_ref() else {
            return;
        };

        let distribution = {
            let ranked_records = ranked_population_control_archive_records(self.archive.as_slice());
            if ranked_records.is_empty() {
                return;
            }

            let elite_vectors = ranked_records
                .iter()
                .take(elite_count.max(1))
                .map(|record| record.parameter_vector.as_slice())
                .collect::<Vec<_>>();

            population_control_distribution_from_elites(
                elite_vectors.as_slice(),
                previous_distribution,
                lower_bounds,
                upper_bounds,
            )
        };

        if let Some(distribution) = distribution {
            self.distribution = Some(distribution);
        }
    }

    fn update_score_snapshots(
        &mut self,
        restart_id: &str,
        score: &PopulationControlScore,
        exact_likelihood_evaluator_counts: Option<
            crate::stages::return_components::ContinuousLikelihoodEvaluatorCounts,
        >,
    ) {
        self.latest_score_snapshot = Some(PopulationControlFitSnapshot {
            restart_id: restart_id.to_string(),
            objective: score.objective(),
            exact_likelihood_evaluator_counts,
        });

        let score_snapshot_improves = best_population_control_score_record(self.archive.as_slice())
            .map(|record| score.compare(&record.score).is_lt())
            .unwrap_or(true);

        if score_snapshot_improves {
            self.best_score_snapshot = Some(PopulationControlFitSnapshot {
                restart_id: restart_id.to_string(),
                objective: score.objective(),
                exact_likelihood_evaluator_counts,
            });
        }

        let global_snapshot_improves = self
            .best_global_snapshot
            .as_ref()
            .map(|snapshot| score.objective() < snapshot.objective)
            .unwrap_or(true);

        if global_snapshot_improves {
            self.best_global_snapshot = Some(PopulationControlFitSnapshot {
                restart_id: restart_id.to_string(),
                objective: score.objective(),
                exact_likelihood_evaluator_counts,
            });
        }
    }

    fn finalization_candidate_vector(
        &self,
        restart_position: usize,
        search_closed: bool,
    ) -> Option<Vec<f64>> {
        let restart_id = self.restart_ids.get(restart_position)?;

        if search_closed
            && let Some(candidate_record) = distinct_ranked_population_control_archive_record(
                self.archive.as_slice(),
                restart_position,
            )
        {
            // Finalization owns candidate vectors because seeded validation runs
            // after archive ranking releases its borrow.
            return Some(candidate_record.parameter_vector.clone());
        }

        if let Some(candidate_record) = best_restart_population_control_archive_record(
            self.archive.as_slice(),
            restart_id.as_str(),
        ) {
            // Finalization owns candidate vectors because seeded validation runs
            // after archive ranking releases its borrow.
            return Some(candidate_record.parameter_vector.clone());
        }

        // The seed fallback owns a vector for the same worker-boundary reason as
        // archive candidates.
        self.seed_vectors.get(restart_position).cloned()
    }
}

#[cfg(test)]
pub(super) fn population_control_state_with_progress_snapshots_for_test(
    latest_objective: f64,
    best_score_objective: f64,
    best_global_objective: f64,
) -> PopulationControlState {
    PopulationControlState {
        restart_ids: vec![
            "latest_restart".to_string(),
            "best_score_restart".to_string(),
            "best_global_restart".to_string(),
        ],
        seed_vectors: vec![vec![0.0], vec![0.0], vec![0.0]],
        distribution: None,
        scheduled_parameter_bits: Vec::new(),
        archive: Vec::new(),
        latest_score_snapshot: Some(PopulationControlFitSnapshot {
            restart_id: "latest_restart".to_string(),
            objective: latest_objective,
            exact_likelihood_evaluator_counts: None,
        }),
        best_score_snapshot: Some(PopulationControlFitSnapshot {
            restart_id: "best_score_restart".to_string(),
            objective: best_score_objective,
            exact_likelihood_evaluator_counts: None,
        }),
        best_global_snapshot: Some(PopulationControlFitSnapshot {
            restart_id: "best_global_restart".to_string(),
            objective: best_global_objective,
            exact_likelihood_evaluator_counts: None,
        }),
        restart_scheduled_search_counts: vec![1, 1, 1],
        restart_completed_search_counts: vec![1, 1, 1],
        max_search_count_per_restart: 1,
        scheduled_search_evaluation_count: 3,
        completed_search_evaluation_count: 3,
        in_flight_search_count: 0,
        scheduled_finalization_count: 0,
        completed_finalization_count: 0,
        seed_candidates_prepared: true,
        next_generation_index: 0,
        next_restart_position: 0,
        search_closed: false,
        stop_reason: POPULATION_CONTROL_STOP_REASON_NO_SCHEDULABLE_WORK.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::workflow::{RestartObjectiveEvaluation, RestartObjectiveResidual};
    use super::*;

    fn test_score(objective: f64) -> PopulationControlScore {
        PopulationControlScore::from_evaluation(&RestartObjectiveEvaluation {
            objective,
            admissibility_warning_count: 0,
            max_standardized_residual: Some(objective),
            exact_likelihood_evaluator_counts: None,
            residuals: vec![RestartObjectiveResidual {
                family_id: "test".to_string(),
                residual_id: "test.residual".to_string(),
                objective_residual: objective,
            }],
        })
    }

    #[test]
    fn open_search_finalization_uses_restart_local_candidate() {
        let mut state = population_control_state_with_progress_snapshots_for_test(3.0, 2.0, 1.0);

        state.archive.push(PopulationControlCandidateRecord {
            restart_id: "other_restart".to_string(),
            parameter_vector: vec![9.0],
            candidate_evaluation_count: 1,
            score: test_score(0.0),
        });

        state.archive.push(PopulationControlCandidateRecord {
            restart_id: "latest_restart".to_string(),
            parameter_vector: vec![1.0],
            candidate_evaluation_count: 1,
            score: test_score(1.0),
        });

        assert_eq!(
            state.finalization_candidate_vector(0, false),
            Some(vec![1.0])
        );
        assert_eq!(
            state.finalization_candidate_vector(0, true),
            Some(vec![9.0])
        );
    }

    #[test]
    fn search_job_scheduling_respects_restart_candidate_budget() {
        let mut state = population_control_state_with_progress_snapshots_for_test(3.0, 2.0, 1.0);

        let candidate_spec = PopulationControlCandidateSpec {
            restart_position: 0,
            restart_id: "latest_restart".to_string(),
            parameter_vector: vec![1.0],
            schedule_category: POPULATION_CONTROL_SCHEDULE_CATEGORY_SAMPLE,
            search_direction_position: Some(0),
            search_direction_label: "population_candidate".to_string(),
        };

        assert!(state.schedule_search_job(candidate_spec).is_none());
    }
}
