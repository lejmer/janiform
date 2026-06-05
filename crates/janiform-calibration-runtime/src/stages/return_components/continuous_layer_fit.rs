use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

#[cfg(test)]
use super::continuous_fit::build_nig_search_candidates;
use super::continuous_fit::{
    ContinuousReturnCandidateEvaluation, OptimizedNigCandidate,
    ReturnComponentsStageContinuousReturnCalibrationSample,
    build_nig_search_candidates_with_progress, evaluate_continuous_nig_candidate,
};
use super::contracts::{
    ReturnComponentsStageCandidateGenerationInputError,
    ReturnComponentsStageObservedSignalSummaryStatistics, ReturnComponentsStageProgressPhase,
    ReturnComponentsStageProgressUpdate,
};
use super::progress::{
    emit_return_components_evaluation_progress, emit_return_components_progress,
};

#[derive(Debug, Clone)]
pub(crate) struct FittedContinuousReturnCandidate {
    pub(crate) nig_configuration: NigSharedPlusShiftConfiguration,
    pub(crate) evaluation: ContinuousReturnCandidateEvaluation,
}

#[derive(Debug, Clone)]
pub(crate) struct FittedContinuousReturnLayer {
    pub(crate) candidates: Vec<FittedContinuousReturnCandidate>,
}

pub(crate) fn fit_continuous_return_layer(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
    solver_settings: &CalibrationOptimizerSettings,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Result<FittedContinuousReturnLayer, ReturnComponentsStageCandidateGenerationInputError> {
    let optimized_candidates = build_nig_search_candidates_with_progress(
        calibration_sample,
        stage_seed,
        solver_settings,
        progress_sender,
    );

    if optimized_candidates.is_empty() {
        return Err(
            ReturnComponentsStageCandidateGenerationInputError::MissingContinuousDistributionSeedCandidate,
        );
    }

    let total_optimized_candidate_count = optimized_candidates.len();

    emit_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ContinuousNigFinalEvaluation,
        0,
        total_optimized_candidate_count,
        Some("evaluate optimized NIG candidates"),
    );

    let fitted_candidates = optimized_candidates
        .into_iter()
        .enumerate()
        .map(
            |(
                optimized_candidate_index,
                OptimizedNigCandidate {
                    configuration: nig_configuration,
                    optimizer_diagnostic,
                },
            )| {
                let mut evaluation = evaluate_continuous_nig_candidate(
                    nig_configuration,
                    calibration_sample,
                    summary_statistics,
                    stage_seed,
                );

                evaluation.fit_breakdown.optimizer_seed_objective =
                    optimizer_diagnostic.seed_objective_score;
                evaluation.fit_breakdown.optimizer_local_objective =
                    optimizer_diagnostic.local_objective_score;
                evaluation.fit_breakdown.optimizer_final_objective =
                    optimizer_diagnostic.final_objective_score;
                evaluation.fit_breakdown.optimizer_local_evaluation_count =
                    optimizer_diagnostic.local_evaluation_count;
                evaluation.fit_breakdown.optimizer_stop_reason =
                    optimizer_diagnostic.stop_reason.to_string();
                evaluation.fit_breakdown.proxy_em_best_objective =
                    optimizer_diagnostic.proxy_em_diagnostic.best_objective;
                evaluation.fit_breakdown.proxy_em_final_convergence_ratio = optimizer_diagnostic
                    .proxy_em_diagnostic
                    .final_convergence_ratio;
                evaluation.fit_breakdown.proxy_em_evaluation_count =
                    optimizer_diagnostic.proxy_em_diagnostic.evaluation_count;
                evaluation.fit_breakdown.proxy_em_completed_seed_count = optimizer_diagnostic
                    .proxy_em_diagnostic
                    .completed_seed_count;
                evaluation.fit_breakdown.proxy_em_refined_seed_count =
                    optimizer_diagnostic.proxy_em_diagnostic.refined_seed_count;
                evaluation.fit_breakdown.proxy_em_requested_seed_count = optimizer_diagnostic
                    .proxy_em_diagnostic
                    .requested_seed_count;
                evaluation.fit_breakdown.proxy_em_iteration_budget_per_seed = optimizer_diagnostic
                    .proxy_em_diagnostic
                    .iteration_budget_per_seed;
                evaluation.fit_breakdown.proxy_em_fitting_sample_count = optimizer_diagnostic
                    .proxy_em_diagnostic
                    .fitting_sample_count;
                evaluation.fit_breakdown.proxy_em_fitting_sample_weight_sum = optimizer_diagnostic
                    .proxy_em_diagnostic
                    .fitting_sample_weight_sum;
                evaluation.fit_breakdown.proxy_em_converged_seed_count = optimizer_diagnostic
                    .proxy_em_diagnostic
                    .converged_seed_count;
                evaluation.fit_breakdown.proxy_em_non_improving_seed_count = optimizer_diagnostic
                    .proxy_em_diagnostic
                    .non_improving_seed_count;
                evaluation
                    .fit_breakdown
                    .proxy_em_numerical_failure_seed_count = optimizer_diagnostic
                    .proxy_em_diagnostic
                    .numerical_failure_seed_count;
                evaluation
                    .fit_breakdown
                    .proxy_em_iteration_budget_exhausted_seed_count = optimizer_diagnostic
                    .proxy_em_diagnostic
                    .iteration_budget_exhausted_seed_count;
                evaluation
                    .fit_breakdown
                    .proxy_em_projection_failure_seed_count = optimizer_diagnostic
                    .proxy_em_diagnostic
                    .projection_failure_seed_count;
                evaluation.fit_breakdown.proxy_em_invalid_initial_seed_count = optimizer_diagnostic
                    .proxy_em_diagnostic
                    .invalid_initial_seed_count;
                evaluation.fit_breakdown.optimizer_basin_diagnostics =
                    optimizer_diagnostic.basin_diagnostics.clone();
                evaluation.optimizer_diagnostic = optimizer_diagnostic;

                let candidate_label = format!("nig_candidate_{:03}", optimized_candidate_index + 1);

                emit_return_components_evaluation_progress(
                    progress_sender,
                    ReturnComponentsStageProgressPhase::ContinuousNigFinalEvaluation,
                    optimized_candidate_index + 1,
                    total_optimized_candidate_count,
                    Some(candidate_label.as_str()),
                    optimized_candidate_index + 1,
                    total_optimized_candidate_count,
                );

                FittedContinuousReturnCandidate {
                    nig_configuration,
                    evaluation,
                }
            },
        )
        .collect();

    Ok(FittedContinuousReturnLayer {
        candidates: fitted_candidates,
    })
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;
    use janiform_foundations::time::NANOS_PER_HOUR;

    use super::{
        build_nig_search_candidates, evaluate_continuous_nig_candidate, fit_continuous_return_layer,
    };
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
                    elapsed_same_session_hours: 1.0,
                    log_return: 1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 2,
                    elapsed_same_session_hours: 1.0,
                    log_return: -1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
            ],
            continuous_fit_returns: vec![
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 1,
                    elapsed_same_session_hours: 1.0,
                    log_return: 1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 2,
                    elapsed_same_session_hours: 1.0,
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
                window_end_utc_nanos: NANOS_PER_HOUR,
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
            None,
        )
        .expect("continuous layer fit should produce a NIG candidate");

        assert!(!fitted_layer.candidates.is_empty());
        assert!(
            fitted_layer
                .candidates
                .iter()
                .all(|candidate| candidate.nig_configuration.delta > 0.0)
        );
        assert!(
            fitted_layer
                .candidates
                .iter()
                .all(|candidate| candidate.evaluation.objective_score.is_finite())
        );
        assert!(fitted_layer.candidates.iter().all(|candidate| {
            candidate
                .evaluation
                .fit_breakdown
                .optimizer_final_objective
                .is_finite()
        }));
    }

    #[test]
    fn continuous_layer_fit_preserves_all_optimized_nig_candidates() {
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
                    elapsed_same_session_hours: 1.0,
                    log_return: 1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.25,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 2,
                    elapsed_same_session_hours: 1.0,
                    log_return: -0.7e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.75,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 3,
                    elapsed_same_session_hours: 1.0,
                    log_return: 0.9e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.35,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 4,
                    elapsed_same_session_hours: 1.0,
                    log_return: -1.1e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.65,
                },
            ],
            continuous_fit_returns: vec![
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 1,
                    elapsed_same_session_hours: 1.0,
                    log_return: 1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.25,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 2,
                    elapsed_same_session_hours: 1.0,
                    log_return: -0.7e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.75,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 3,
                    elapsed_same_session_hours: 1.0,
                    log_return: 0.9e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.35,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 4,
                    elapsed_same_session_hours: 1.0,
                    log_return: -1.1e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.65,
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
                window_end_utc_nanos: NANOS_PER_HOUR,
            },
        };

        let calibration_sample = continuous_fit::build_continuous_return_calibration_sample(
            &sorted_signals,
            &[1.0, 1.2, 0.8, 1.1],
            summary_statistics.open_session_sell_share_proxy,
        );
        let solver_settings =
            CalibrationOptimizerSettings::try_new("adaptive_search", 32, 1, 1.0e-8, 1.0e-8, 1.0)
                .expect("solver settings should validate");

        let optimized_candidates =
            build_nig_search_candidates(&calibration_sample, 17, &solver_settings);
        let fitted_layer = fit_continuous_return_layer(
            summary_statistics,
            &calibration_sample,
            17,
            &solver_settings,
            None,
        )
        .expect("continuous layer fit should preserve optimized candidates");

        assert!(optimized_candidates.len() > 1);
        assert_eq!(fitted_layer.candidates.len(), optimized_candidates.len());
        for (optimized_candidate, fitted_candidate) in optimized_candidates
            .iter()
            .zip(fitted_layer.candidates.iter())
        {
            let exact_evaluation = evaluate_continuous_nig_candidate(
                fitted_candidate.nig_configuration,
                &calibration_sample,
                summary_statistics,
                17,
            );

            assert_eq!(
                fitted_candidate.evaluation.objective_score.to_bits(),
                exact_evaluation.objective_score.to_bits()
            );
            assert_eq!(
                fitted_candidate
                    .evaluation
                    .fit_breakdown
                    .optimizer_final_objective
                    .to_bits(),
                optimized_candidate
                    .optimizer_diagnostic
                    .final_objective_score
                    .to_bits()
            );
            assert!(
                fitted_candidate
                    .evaluation
                    .fit_breakdown
                    .proxy_em_best_objective
                    .is_some()
            );
            assert!(
                fitted_candidate
                    .evaluation
                    .fit_breakdown
                    .proxy_em_evaluation_count
                    > 0
            );
            assert_eq!(
                fitted_candidate
                    .evaluation
                    .fit_breakdown
                    .proxy_em_completed_seed_count,
                fitted_candidate
                    .evaluation
                    .fit_breakdown
                    .proxy_em_requested_seed_count,
            );
            assert_eq!(
                fitted_candidate
                    .evaluation
                    .fit_breakdown
                    .proxy_em_converged_seed_count
                    + fitted_candidate
                        .evaluation
                        .fit_breakdown
                        .proxy_em_non_improving_seed_count
                    + fitted_candidate
                        .evaluation
                        .fit_breakdown
                        .proxy_em_numerical_failure_seed_count
                    + fitted_candidate
                        .evaluation
                        .fit_breakdown
                        .proxy_em_iteration_budget_exhausted_seed_count
                    + fitted_candidate
                        .evaluation
                        .fit_breakdown
                        .proxy_em_invalid_initial_seed_count,
                fitted_candidate
                    .evaluation
                    .fit_breakdown
                    .proxy_em_requested_seed_count,
            );
        }
    }
}
