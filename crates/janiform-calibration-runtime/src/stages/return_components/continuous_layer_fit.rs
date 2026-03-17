use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::continuous_fit::{
    ContinuousReturnCandidateEvaluation, ReturnComponentsStageContinuousReturnCalibrationSample,
    build_nig_search_candidates, evaluate_continuous_nig_candidate,
};
use super::contracts::ReturnComponentsStageObservedSignalSummaryStatistics;

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
) -> FittedContinuousReturnLayer {
    let nig_candidates = build_nig_search_candidates(
        summary_statistics,
        calibration_sample,
        stage_seed,
        solver_settings,
    );
    let mut fitted_layer: Option<FittedContinuousReturnLayer> = None;

    for nig_configuration in nig_candidates {
        let evaluation = evaluate_continuous_nig_candidate(
            nig_configuration,
            calibration_sample,
            summary_statistics,
            stage_seed,
        );
        let candidate_layer = FittedContinuousReturnLayer {
            nig_configuration,
            evaluation,
        };
        let should_replace = fitted_layer
            .as_ref()
            .map(|current_best| {
                candidate_layer.evaluation.objective_score < current_best.evaluation.objective_score
            })
            .unwrap_or(true);
        if should_replace {
            fitted_layer = Some(candidate_layer);
        }
    }

    fitted_layer.expect("continuous-layer search must yield at least one NIG candidate")
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;

    use super::fit_continuous_return_layer;
    use crate::stages::return_components::{
        ReturnComponentsObservedOpenSessionReturn,
        ReturnComponentsStageObservedSignalSummaryStatistics,
        ReturnComponentsStageSignalSourceWindow, ReturnComponentsStageSortedSignalInputs,
        continuous_fit,
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
                window_end_utc_nanos: 3_600_000_000_000,
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
        );

        assert!(fitted_layer.nig_configuration.delta > 0.0);
        assert!(fitted_layer.evaluation.objective_score.is_finite());
    }

    fn test_summary_statistics() -> ReturnComponentsStageObservedSignalSummaryStatistics {
        ReturnComponentsStageObservedSignalSummaryStatistics {
            known_calendar_event_count: 0,
            known_realized_sample_count: 0,
            source_window_duration_hours: 1.0,
            observed_open_session_return_count: 0,
            continuous_fit_return_count: 0,
            intraday_jump_candidate_count: 0,
            intraday_jump_episode_count: 0,
            selected_surprise_jump_episode_count: 0,
            close_open_gap_candidate_count: 0,
            close_open_gap_episode_count: 0,
            open_session_sell_share_proxy: 0.5,
            mean_continuous_business_time_scale: 1.0,
            continuous_return_mean: 0.0,
            continuous_return_volatility: 0.0,
            continuous_return_skewness: 0.0,
            continuous_return_excess_kurtosis: 0.0,
            observed_log_price_span: 0.0,
            mean_abs_known_open_jump_return: 0.0,
            mean_abs_known_closed_gap_return: 0.0,
            mean_abs_surprise_open_jump_return: 0.0,
            mean_abs_surprise_closed_gap_return: 0.0,
            mean_known_post_shock_activity_lift: 0.0,
            mean_surprise_post_shock_activity_lift: 0.0,
            known_activity_coupling_target: 0.0,
            surprise_activity_coupling_target: 0.0,
            known_news_coverage_ratio: 0.0,
            surprise_arrival_intensity_per_hour: 0.0,
        }
    }
}
