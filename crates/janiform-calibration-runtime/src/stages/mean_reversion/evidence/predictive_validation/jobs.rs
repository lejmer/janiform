use crate::stages::mean_reversion::{
    FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY, FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY,
    MeanReversionGridCandidateEvaluation, MeanReversionStageCandidateEvaluation,
    OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY, observed_pressure_replay,
};

use super::schedule::{
    minimum_training_observation_count, rolling_origin_refit_training_observation_counts,
};

pub(super) struct PredictiveEvidenceJob<'a> {
    pub(super) grid_index: usize,
    pub(super) candidate_index: usize,
    pub(super) grid_evaluation: &'a MeanReversionGridCandidateEvaluation,
    pub(super) candidate: &'a MeanReversionStageCandidateEvaluation,
    pub(super) progress_label: String,
    pub(super) progress_unit_count: usize,
}

pub(super) fn predictive_evidence_jobs(
    grid_evaluations: &[MeanReversionGridCandidateEvaluation],
) -> Vec<PredictiveEvidenceJob<'_>> {
    let mut jobs = Vec::new();

    for (grid_index, grid_evaluation) in grid_evaluations.iter().enumerate() {
        for (candidate_index, candidate) in grid_evaluation.evaluated_candidates.iter().enumerate()
        {
            if !candidate_supports_predictive_evidence(candidate) {
                continue;
            }

            let refit_count = rolling_origin_refit_training_observation_counts(
                grid_evaluation.observation_levels.len(),
                minimum_training_observation_count(candidate.candidate_family),
            )
            .len();

            jobs.push(PredictiveEvidenceJob {
                grid_index,
                candidate_index,
                grid_evaluation,
                candidate,
                progress_label: format!(
                    "{}:{}",
                    grid_evaluation.calibration_grid.calibration_grid_kind,
                    candidate.candidate_family
                ),
                progress_unit_count: refit_count.max(1),
            });
        }
    }

    jobs
}

fn candidate_supports_predictive_evidence(
    candidate: &MeanReversionStageCandidateEvaluation,
) -> bool {
    match candidate.candidate_family {
        FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY | FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY => {
            candidate.kappa > 0.0
        }
        OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY => {
            observed_pressure_replay::observed_pressure_replay_pressure_channel_is_non_null(
                candidate.least_squares_rank,
                candidate.anchor_model.pressure_level_gain,
                candidate
                    .anchor_model
                    .pressure_persistence_per_reference_interval,
            )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::MeanReversionStageCalibrationGrid;
    use janiform_model_kernels::mean_reversion::MeanReversionAnchorModel;

    use crate::stages::mean_reversion::{
        FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY, FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY,
        MeanReversionGridCandidateEvaluation, MeanReversionStageCandidateEvaluation,
        OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY, RANDOM_WALK_NULL_CANDIDATE_FAMILY,
    };

    use super::{candidate_supports_predictive_evidence, predictive_evidence_jobs};

    fn candidate_evaluation(
        candidate_family: &'static str,
        kappa: f64,
        pressure_level_gain: f64,
    ) -> MeanReversionStageCandidateEvaluation {
        let residuals = vec![0.0; 8];

        MeanReversionStageCandidateEvaluation {
            candidate_family,
            anchor_model: MeanReversionAnchorModel::endogenous_core_pressure_level(
                1.0,
                0.5,
                pressure_level_gain,
            ),
            anchor_origin_level: Some(0.0),
            phi_reference: 1.0,
            kappa,
            residual_sum_of_squares: 1.0,
            quasi_log_likelihood: 0.0,
            information_criterion_bic: 0.0,
            parameter_count: 3,
            least_squares_rank: 1,
            pressure_state_path: vec![0.0; residuals.len().saturating_add(1)],
            boundary_anchor_values: vec![0.0; residuals.len().saturating_add(1)],
            deviations: vec![0.0; residuals.len().saturating_add(1)],
            residuals,
            predictive_evidence: None,
            stage_decision_reason: "predictive_evidence_fixture.v1".to_string(),
        }
    }

    fn grid_evaluation(
        evaluated_candidates: Vec<MeanReversionStageCandidateEvaluation>,
    ) -> MeanReversionGridCandidateEvaluation {
        MeanReversionGridCandidateEvaluation {
            calibration_grid: MeanReversionStageCalibrationGrid {
                calibration_grid_kind: "predictive_evidence_fixture_grid.v1".to_string(),
                calibration_grid_interval_hours: 1.0,
                source_observation_count: 9,
                retained_observation_count: 9,
                coverage_ratio: 1.0,
            },
            observation_levels: vec![0.0; 9],
            calibration_context: None,
            evaluated_candidates,
            stage_decision_reason: "predictive_evidence_fixture_grid_evaluated.v1".to_string(),
        }
    }

    #[test]
    fn predictive_evidence_supports_zero_kappa_observed_pressure_replay() {
        let observed_pressure_candidate =
            candidate_evaluation(OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY, 0.0, 0.5);

        let zero_gain_observed_pressure_candidate =
            candidate_evaluation(OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY, 0.0, 0.0);

        let flat_anchor_candidate =
            candidate_evaluation(FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY, 0.0, 0.5);

        let positive_kappa_candidate =
            candidate_evaluation(FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY, 0.25, 0.5);

        let random_walk_candidate =
            candidate_evaluation(RANDOM_WALK_NULL_CANDIDATE_FAMILY, 0.0, 0.5);

        assert!(candidate_supports_predictive_evidence(
            &observed_pressure_candidate
        ));
        assert!(!candidate_supports_predictive_evidence(
            &zero_gain_observed_pressure_candidate
        ));
        assert!(!candidate_supports_predictive_evidence(
            &flat_anchor_candidate
        ));
        assert!(candidate_supports_predictive_evidence(
            &positive_kappa_candidate
        ));
        assert!(!candidate_supports_predictive_evidence(
            &random_walk_candidate
        ));

        let grid_evaluation = grid_evaluation(vec![
            observed_pressure_candidate,
            zero_gain_observed_pressure_candidate,
            flat_anchor_candidate,
            positive_kappa_candidate,
            random_walk_candidate,
        ]);

        let grid_evaluations = [grid_evaluation];
        let jobs = predictive_evidence_jobs(&grid_evaluations);

        let admitted_candidate_families = jobs
            .iter()
            .map(|job| job.candidate.candidate_family)
            .collect::<Vec<_>>();

        assert_eq!(
            admitted_candidate_families,
            vec![
                OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY,
                FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY
            ]
        );
    }
}
