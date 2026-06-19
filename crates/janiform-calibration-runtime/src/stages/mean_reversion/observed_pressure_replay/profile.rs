use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::linear_algebra::{
    RankAwareLeastSquaresSolution, rank_aware_unit_interval_gain_least_squares,
};
use janiform_model_kernels::mean_reversion::MeanReversionAnchorModel;

use super::boundary_replay::{observed_pressure_gain_fit_terms, observed_pressure_replay_path};
use super::failure::observed_pressure_replay_failure;
use crate::stages::mean_reversion::{
    MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure,
    OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY,
    OBSERVED_PRESSURE_REPLAY_IDENTIFIABLE_PARAMETER_COUNT, OBSERVED_PRESSURE_REPLAY_REASON, bic,
    gaussian_quasi_log_likelihood, kappa_from_phi_reference,
    profile_candidate_over_pressure_persistence, profile_candidate_over_unit_interval,
    stage_decision_reason_with_boundary_flags,
};

const OBSERVED_PRESSURE_REPLAY_DRIVER_POLICY: &str =
    "causal_observed_core_return_pressure_driver_replay.v1";
const OBSERVED_PRESSURE_REPLAY_SUPPORT_POLICY: &str =
    "rank_aware_least_squares_pressure_column_support.v1";
const OBSERVED_PRESSURE_REPLAY_NONZERO_COLUMN_REASON: &str =
    "observed_pressure_replay_pressure_column_supported.v1";
const OBSERVED_PRESSURE_REPLAY_ZERO_COLUMN_REASON: &str =
    "observed_pressure_replay_pressure_column_unidentifiable_zero_support.v1";
const OBSERVED_PRESSURE_REPLAY_NONZERO_SUPPORT_REASON: &str =
    "observed_pressure_replay_pressure_driver_supported.v1";
const OBSERVED_PRESSURE_REPLAY_ZERO_SUPPORT_REASON: &str =
    "observed_pressure_replay_pressure_driver_zero_gain_or_unit_persistence_boundary.v1";

// Build the pressure support column before the gain is fitted, so the fitted
// pressure gain cannot feed back into its own explanatory driver.
const OBSERVED_PRESSURE_REPLAY_SUPPORT_FIT_PRESSURE_LEVEL_GAIN: f64 = 0.0;

pub(in crate::stages::mean_reversion) fn observed_pressure_replay_pressure_channel_is_non_null(
    least_squares_rank: usize,
    pressure_level_gain: f64,
    pressure_persistence_per_reference_interval: f64,
) -> bool {
    least_squares_rank > 0
        && pressure_level_gain > 0.0
        && pressure_persistence_per_reference_interval < 1.0
}

pub(in crate::stages::mean_reversion) fn profile_causal_observed_pressure_replay_candidate(
    observation_levels: &[f64],
    calibration_grid_interval_hours: f64,
    interval_deltas: &[f64],
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure> {
    profile_candidate_over_pressure_persistence(solver_settings, |pressure_persistence| {
        let phi_boundary_candidate = evaluate_causal_observed_pressure_replay_candidate(
            observation_levels,
            calibration_grid_interval_hours,
            interval_deltas,
            pressure_persistence,
            1.0,
        )?;

        profile_candidate_over_unit_interval(
            solver_settings,
            phi_boundary_candidate,
            |phi_reference| {
                evaluate_causal_observed_pressure_replay_candidate(
                    observation_levels,
                    calibration_grid_interval_hours,
                    interval_deltas,
                    pressure_persistence,
                    phi_reference,
                )
            },
        )
    })
}

fn evaluate_causal_observed_pressure_replay_candidate(
    observation_levels: &[f64],
    calibration_grid_interval_hours: f64,
    interval_deltas: &[f64],
    pressure_persistence_per_reference_interval: f64,
    phi_reference: f64,
) -> Result<MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure> {
    validate_observed_pressure_replay_inputs(observation_levels, interval_deltas)?;

    let zero_gain_anchor_model = MeanReversionAnchorModel::endogenous_core_pressure_level(
        calibration_grid_interval_hours,
        pressure_persistence_per_reference_interval,
        OBSERVED_PRESSURE_REPLAY_SUPPORT_FIT_PRESSURE_LEVEL_GAIN,
    );

    let pressure_gain_fit_terms = observed_pressure_gain_fit_terms(
        observation_levels,
        interval_deltas,
        &zero_gain_anchor_model,
        phi_reference,
    )?;

    let Some(least_squares_solution) = rank_aware_unit_interval_gain_least_squares(
        pressure_gain_fit_terms.pressure_column.as_slice(),
        pressure_gain_fit_terms.response.as_slice(),
    ) else {
        return observed_pressure_replay_failure(
            "observed pressure replay least-squares requires non-empty aligned finite inputs",
        );
    };

    let anchor_model = MeanReversionAnchorModel::endogenous_core_pressure_level(
        calibration_grid_interval_hours,
        pressure_persistence_per_reference_interval,
        least_squares_solution.first_coefficient,
    );

    let replay_path = observed_pressure_replay_path(
        observation_levels,
        interval_deltas,
        &anchor_model,
        phi_reference,
    )?;

    let residual_sum_of_squares = replay_path
        .residuals
        .iter()
        .map(|residual| residual * residual)
        .sum::<f64>();

    let quasi_log_likelihood =
        gaussian_quasi_log_likelihood(replay_path.residuals.len(), residual_sum_of_squares);

    Ok(MeanReversionStageCandidateEvaluation {
        candidate_family: OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY,
        anchor_model,
        anchor_origin_level: observation_levels.first().copied(),
        phi_reference,
        kappa: kappa_from_phi_reference(phi_reference, calibration_grid_interval_hours),
        residual_sum_of_squares,
        quasi_log_likelihood,
        information_criterion_bic: bic(
            replay_path.residuals.len(),
            OBSERVED_PRESSURE_REPLAY_IDENTIFIABLE_PARAMETER_COUNT,
            quasi_log_likelihood,
        ),
        parameter_count: OBSERVED_PRESSURE_REPLAY_IDENTIFIABLE_PARAMETER_COUNT,
        least_squares_rank: least_squares_solution.rank,
        pressure_state_path: replay_path.pressure_state_path,
        boundary_anchor_values: replay_path.boundary_anchor_values,
        deviations: replay_path.deviations,
        residuals: replay_path.residuals,
        predictive_evidence: None,
        stage_decision_reason: observed_pressure_replay_stage_decision_reason(
            pressure_persistence_per_reference_interval,
            phi_reference,
            least_squares_solution,
        ),
    })
}

fn validate_observed_pressure_replay_inputs(
    observation_levels: &[f64],
    interval_deltas: &[f64],
) -> Result<(), MeanReversionStageExecutionFailure> {
    let transition_count = observation_levels.len().saturating_sub(1);

    if observation_levels.is_empty() {
        return observed_pressure_replay_failure(
            "observed pressure replay requires at least one observation level",
        );
    }

    if interval_deltas.len() != transition_count {
        return observed_pressure_replay_failure(
            "observed pressure replay requires one interval delta per observation transition",
        );
    }

    Ok(())
}

fn observed_pressure_replay_stage_decision_reason(
    pressure_persistence_per_reference_interval: f64,
    phi_reference: f64,
    least_squares_solution: RankAwareLeastSquaresSolution,
) -> String {
    let mut reason = stage_decision_reason_with_boundary_flags(
        OBSERVED_PRESSURE_REPLAY_REASON,
        Some(pressure_persistence_per_reference_interval),
        Some(phi_reference),
    );

    reason.push('|');
    reason.push_str(OBSERVED_PRESSURE_REPLAY_DRIVER_POLICY);
    reason.push('|');
    reason.push_str(OBSERVED_PRESSURE_REPLAY_SUPPORT_POLICY);
    reason.push('|');

    if least_squares_solution.rank == 0 {
        reason.push_str(OBSERVED_PRESSURE_REPLAY_ZERO_COLUMN_REASON);
    } else {
        reason.push_str(OBSERVED_PRESSURE_REPLAY_NONZERO_COLUMN_REASON);
    }

    reason.push('|');

    if observed_pressure_replay_pressure_channel_is_non_null(
        least_squares_solution.rank,
        least_squares_solution.first_coefficient,
        pressure_persistence_per_reference_interval,
    ) {
        reason.push_str(OBSERVED_PRESSURE_REPLAY_NONZERO_SUPPORT_REASON);
    } else {
        reason.push_str(OBSERVED_PRESSURE_REPLAY_ZERO_SUPPORT_REASON);
    }

    reason
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;

    use super::super::boundary_replay::{
        observed_pressure_replay_boundary_state_at_candidate_end,
        predict_causal_observed_pressure_replay_next_observation,
        update_causal_observed_pressure_replay_boundary_state,
    };
    use super::{
        OBSERVED_PRESSURE_REPLAY_NONZERO_COLUMN_REASON,
        OBSERVED_PRESSURE_REPLAY_NONZERO_SUPPORT_REASON,
        OBSERVED_PRESSURE_REPLAY_ZERO_COLUMN_REASON, OBSERVED_PRESSURE_REPLAY_ZERO_SUPPORT_REASON,
        evaluate_causal_observed_pressure_replay_candidate,
    };
    use crate::stages::mean_reversion::OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY;

    fn solver_settings() -> CalibrationOptimizerSettings {
        CalibrationOptimizerSettings::try_new("golden-section", 32, 1, 1.0e-8, 1.0e-6, 0.0)
            .expect("solver settings should validate")
    }

    #[test]
    fn observed_pressure_replay_reports_nonzero_pressure_support() {
        let observation_levels = [0.0, 0.02, -0.01, 0.03, -0.02];
        let interval_deltas = [1.0; 4];

        let candidate = evaluate_causal_observed_pressure_replay_candidate(
            observation_levels.as_slice(),
            1.0,
            interval_deltas.as_slice(),
            0.5,
            0.8,
        )
        .expect("observed pressure replay candidate should evaluate");

        assert_eq!(candidate.least_squares_rank, 1);
        assert!(
            candidate
                .pressure_state_path
                .iter()
                .any(|pressure_state| *pressure_state != 0.0)
        );
        assert!(
            candidate
                .stage_decision_reason
                .contains(OBSERVED_PRESSURE_REPLAY_NONZERO_COLUMN_REASON)
        );
        assert!(
            candidate
                .stage_decision_reason
                .contains(OBSERVED_PRESSURE_REPLAY_NONZERO_SUPPORT_REASON)
        );
    }

    #[test]
    fn observed_pressure_replay_reports_zero_pressure_support() {
        let observation_levels = [1.0, 1.0, 1.0, 1.0];
        let interval_deltas = [1.0; 3];

        let candidate = evaluate_causal_observed_pressure_replay_candidate(
            observation_levels.as_slice(),
            1.0,
            interval_deltas.as_slice(),
            0.5,
            0.8,
        )
        .expect("observed pressure replay candidate should evaluate");

        assert_eq!(candidate.least_squares_rank, 0);
        assert_eq!(candidate.anchor_model.pressure_level_gain, 0.0);
        assert!(
            candidate
                .stage_decision_reason
                .contains(OBSERVED_PRESSURE_REPLAY_ZERO_COLUMN_REASON)
        );
        assert!(
            candidate
                .stage_decision_reason
                .contains(OBSERVED_PRESSURE_REPLAY_ZERO_SUPPORT_REASON)
        );
    }

    #[test]
    fn observed_pressure_replay_prediction_uses_state_before_next_observation() {
        let observation_levels = [0.0, 0.02, -0.01, 0.03];
        let interval_deltas = [1.0; 3];

        let candidate = evaluate_causal_observed_pressure_replay_candidate(
            observation_levels.as_slice(),
            1.0,
            interval_deltas.as_slice(),
            0.5,
            0.8,
        )
        .expect("observed pressure replay candidate should evaluate");

        let current_observation = *observation_levels
            .last()
            .expect("fixture should have a current observation");

        let boundary_state = observed_pressure_replay_boundary_state_at_candidate_end(
            &candidate,
            current_observation,
        )
        .expect("candidate should expose its terminal boundary state");

        let prediction_before_first_next =
            predict_causal_observed_pressure_replay_next_observation(
                &boundary_state,
                &candidate,
                1.0,
            )
            .expect("prediction should evaluate");

        let prediction_before_second_next =
            predict_causal_observed_pressure_replay_next_observation(
                &boundary_state,
                &candidate,
                1.0,
            )
            .expect("prediction should evaluate");

        assert_eq!(prediction_before_first_next, prediction_before_second_next);

        let first_updated_state = update_causal_observed_pressure_replay_boundary_state(
            boundary_state.clone(),
            &candidate,
            1.0,
            0.04,
        )
        .expect("first update should evaluate");

        let second_updated_state = update_causal_observed_pressure_replay_boundary_state(
            boundary_state,
            &candidate,
            1.0,
            -0.04,
        )
        .expect("second update should evaluate");

        assert_ne!(
            first_updated_state.pressure_state,
            second_updated_state.pressure_state
        );
    }

    #[test]
    fn observed_pressure_replay_profile_evaluates_with_solver_settings() {
        let observation_levels = [0.0, 0.02, -0.01, 0.03, -0.02];
        let interval_deltas = [1.0; 4];

        let candidate = super::profile_causal_observed_pressure_replay_candidate(
            observation_levels.as_slice(),
            1.0,
            interval_deltas.as_slice(),
            &solver_settings(),
        )
        .expect("observed pressure replay profile should evaluate");

        assert_eq!(
            candidate.candidate_family,
            OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY
        );
    }
}
