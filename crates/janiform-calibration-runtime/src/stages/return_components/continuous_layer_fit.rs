use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::continuous_fit::{
    ContinuousReturnCandidateEvaluation, OptimizedNigCandidate,
    ReturnComponentsStageContinuousReturnCalibrationSample, build_nig_search_candidates,
    evaluate_continuous_nig_candidate,
};
use super::contracts::{
    ReturnComponentsStageCandidateGenerationInputError,
    ReturnComponentsStageObservedSignalSummaryStatistics,
};

#[derive(Debug, Clone)]
pub(crate) struct FittedContinuousReturnLayer {
    pub(crate) nig_configuration: NigSharedPlusShiftConfiguration,
    pub(crate) evaluation: ContinuousReturnCandidateEvaluation,
}

pub(crate) fn fit_continuous_return_layer(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<FittedContinuousReturnLayer, ReturnComponentsStageCandidateGenerationInputError> {
    let optimized_candidates =
        build_nig_search_candidates(calibration_sample, stage_seed, solver_settings);

    let OptimizedNigCandidate {
        configuration: nig_configuration,
        optimizer_diagnostic,
    } = optimized_candidates
        .into_iter()
        .next()
        .ok_or(
            ReturnComponentsStageCandidateGenerationInputError::MissingContinuousDistributionSeedCandidate,
        )?;

    let mut evaluation = evaluate_continuous_nig_candidate(
        nig_configuration,
        calibration_sample,
        summary_statistics,
        stage_seed,
    );

    evaluation.objective_score = optimizer_diagnostic.final_objective_score;
    evaluation.fit_breakdown.optimizer_seed_objective = optimizer_diagnostic.seed_objective_score;
    evaluation.fit_breakdown.optimizer_local_objective = optimizer_diagnostic.local_objective_score;
    evaluation.fit_breakdown.optimizer_final_objective = optimizer_diagnostic.final_objective_score;
    evaluation.fit_breakdown.optimizer_local_evaluation_count =
        optimizer_diagnostic.local_evaluation_count;
    evaluation.fit_breakdown.optimizer_stop_reason = optimizer_diagnostic.stop_reason.to_string();
    evaluation.fit_breakdown.optimizer_basin_diagnostics =
        optimizer_diagnostic.basin_diagnostics.clone();
    evaluation.optimizer_diagnostic = optimizer_diagnostic;

    Ok(FittedContinuousReturnLayer {
        nig_configuration,
        evaluation,
    })
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;
    use janiform_foundations::time::ONE_HOUR_NANOS;

    use super::fit_continuous_return_layer;
    use crate::stages::return_components::{
        ReturnComponentsObservedOpenSessionReturn,
        ReturnComponentsStageObservedSignalSummaryStatistics,
        ReturnComponentsStageSignalSourceWindow, continuous_fit,
        execution::ReturnComponentsStageSortedSignalInputs, test_support::test_summary_statistics,
    };

    #[test]
    fn continuous_layer_fit_returns_a_positive_delta_configuration() {
        let summary_statistics = ReturnComponentsStageObservedSignalSummaryStatistics {
            open_session_sell_share_proxy: 0.5,
            continuous_fit_return_count: 2,
            continuous_return_volatility: 1.0e-6,
            ..test_summary_statistics()
        };

        let sorted_signals = ReturnComponentsStageSortedSignalInputs {
            known_return_samples: Vec::new(),
            observed_open_session_returns: vec![
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 1,
                    log_return: 1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 2,
                    log_return: -1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
            ],
            continuous_fit_returns: vec![
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 1,
                    log_return: 1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 2,
                    log_return: -1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
            ],
            intraday_jump_candidates: Vec::new(),
            surprise_jump_episodes: Vec::new(),
            selected_surprise_jump_episodes: Vec::new(),
            close_open_gap_candidates: Vec::new(),
            surprise_open_jump_magnitudes: Vec::new(),
            combined_gap_magnitudes: Vec::new(),
            source_window: ReturnComponentsStageSignalSourceWindow {
                window_start_utc_nanos: 0,
                window_end_utc_nanos: ONE_HOUR_NANOS,
            },
        };

        let calibration_sample = continuous_fit::build_continuous_return_calibration_sample(
            &sorted_signals,
            &[1.0, 1.0],
            summary_statistics.open_session_sell_share_proxy,
        );

        let solver_settings =
            CalibrationOptimizerSettings::try_new("adaptive_search", 32, 1, 1.0e-8, 1.0e-8, 1.0)
                .expect("solver settings should validate");

        let fitted_layer = fit_continuous_return_layer(
            summary_statistics,
            &calibration_sample,
            17,
            &solver_settings,
        )
        .expect("continuous layer fit should produce a NIG candidate");

        assert!(fitted_layer.nig_configuration.delta > 0.0);
        assert!(fitted_layer.evaluation.objective_score.is_finite());
        assert!(
            fitted_layer
                .evaluation
                .fit_breakdown
                .optimizer_final_objective
                .is_finite()
        );
    }
}
