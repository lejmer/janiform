use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, ReturnComponentsStageFittedParameters,
};
use janiform_foundations::numeric::sample_standard_deviation;
use janiform_foundations::time::utc_nanos;
use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::super::contracts::{
    ReturnComponentsActivityCouplingCandidateFamily, ReturnComponentsCloseOpenGapDetectionPolicy,
    ReturnComponentsDetectedIntradayJumpCandidate, ReturnComponentsKnownNewsCalendarInput,
    ReturnComponentsObservedCloseOpenBoundaryObservation,
    ReturnComponentsObservedOpenSessionReturn, ReturnComponentsRealizedKnownNewsReturnSample,
    ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageCandidateGenerationInputError,
    ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
    ReturnComponentsStageExactLikelihoodFailureDiagnostic,
    ReturnComponentsStageExactLikelihoodFailureReason,
    ReturnComponentsStageExactLikelihoodFailureScope, ReturnComponentsStageObjectiveMetricWeights,
    ReturnComponentsStageObservedSignalSummaryStatistics, ReturnComponentsStageSignalSourceWindow,
    ReturnComponentsSurpriseNewsArrivalCandidateFamily,
};
use super::super::derived_contracts::return_components_stage_positive_domain_floor;
use super::super::diagnostic_policy_fit;
use super::super::objective_scoring_policy_fit;
use super::super::price_range_policy_fit;
use super::super::signal_partition_fit;
use super::super::surprise_episode_selection_fit;
use super::candidate_generation::generate_return_components_stage_candidate_inputs;
use super::diagnostics::collect_return_components_exact_likelihood_constraint_violations;
use super::{
    ReturnComponentsStageSortedSignalInputs, build_activity_coupling_diagnostic,
    build_gap_diagnostic, build_jump_diagnostic, build_price_range_sanity_diagnostic,
    build_surprise_arrival_diagnostic, default_surprise_deterministic_intensity_profile,
    derive_close_open_gap_candidates, evaluate_return_components_exact_candidate,
    sort_candidate_generation_signals, summarize_candidate_generation_signals,
    summarize_return_distribution,
};
use crate::stages::return_components::continuous_fit;
use janiform_market_time::calendar::ExchangeCalendarRegistry;
use janiform_market_time::calendar::ExchangeCalendarRegistryLoader;

#[test]
fn derived_close_open_gap_detection_filters_zero_gap_boundaries() {
    let calendar_registry = test_calendar_registry();

    let derived_candidates = derive_close_open_gap_candidates(
        "XNYS",
        &calendar_registry,
        test_source_window(),
        &[ReturnComponentsObservedCloseOpenBoundaryObservation {
            close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 21, 0, 0),
            close_price: 100.0,
            open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
            open_price: 100.0,
            post_shock_activity_lift: 1.25,
        }],
        ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
    )
    .expect("zero-gap boundary should validate");

    assert!(derived_candidates.is_empty());
}

#[test]
fn exact_likelihood_structural_failures_are_treated_as_constraint_violations() {
    let fit_breakdown = ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
        global_fit_score: f64::NAN,
        conditional_fit_score: 0.0,
        boundary_penalty: 0.0,
        profile_fit_score: 0.5,
        log_likelihood_shift: 0.0,
        optimizer_seed_objective: 0.0,
        optimizer_local_objective: 0.0,
        optimizer_final_objective: 0.0,
        optimizer_local_evaluation_count: 0,
        optimizer_stop_reason: "test".to_string(),
        optimizer_basin_diagnostics: Vec::new(),
        exact_likelihood_failures: vec![ReturnComponentsStageExactLikelihoodFailureDiagnostic {
            scope: ReturnComponentsStageExactLikelihoodFailureScope::Global,
            scale_bucket_index: None,
            block_member_index: Some(3),
            failure_reason:
                ReturnComponentsStageExactLikelihoodFailureReason::InvalidBlockStandardDeviation,
            evaluator_kind: None,
            observed_return: Some(0.0),
            business_time_scale: Some(1.0),
            sell_share_proxy: Some(0.5),
            integrated_buy_intensity: Some(0.5),
            integrated_sell_intensity: Some(0.5),
            block_standard_deviation: Some(0.0),
            standardized_return: None,
            tilt_theta: None,
            tilt_domain_lower_theta_exclusive: None,
            tilt_domain_upper_theta_exclusive: None,
            standardized_decay_scale: None,
            integration_upper_bound: None,
            log_density_resolution: None,
            scope_average_log_likelihood_error_upper_bound: None,
        }],
        exact_closed_form_single_side_buy_block_count: 0,
        exact_closed_form_single_side_sell_block_count: 0,
        exact_tilted_mixed_inversion_block_count: 0,
        exact_structural_failure_count: 1,
        exact_uncertified_scope_count: 0,
        exact_global_average_log_likelihood_error_upper_bound: None,
        exact_conditional_average_log_likelihood_error_upper_bound: None,
        mean_return_fit: 0.0,
        volatility_fit: 1.0,
        median_return_fit: 0.0,
        lower_quartile_fit: 0.0,
        upper_quartile_fit: 0.0,
        lower_tail_q05_fit: 0.0,
        upper_tail_q95_fit: 0.0,
        extreme_left_tail_q01_fit: 0.0,
        extreme_right_tail_q99_fit: 0.0,
        mean_abs_return_fit: 0.0,
        median_abs_return_fit: 0.0,
        abs_quantile_ratio_fit: 0.0,
        bowley_skewness_fit: 0.0,
        l_moment_lambda2_fit: 0.0,
        l_moment_tau3_fit: 0.0,
    };

    let violations =
        collect_return_components_exact_likelihood_constraint_violations(&fit_breakdown);

    assert_eq!(violations.len(), 1);
    assert_eq!(
        violations[0].field,
        "continuous_distribution_fit_breakdown.exact_likelihood_failures"
    );
    assert!(
        violations[0]
            .message
            .contains("InvalidBlockStandardDeviation")
    );
}

#[test]
fn derived_close_open_gap_detection_emits_overnight_gap_candidate() {
    let calendar_registry = test_calendar_registry();

    let derived_candidates = derive_close_open_gap_candidates(
        "XNYS",
        &calendar_registry,
        test_source_window(),
        &[ReturnComponentsObservedCloseOpenBoundaryObservation {
            close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 21, 0, 0),
            close_price: 100.0,
            open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
            open_price: 100.0 * (-0.013_f64).exp(),
            post_shock_activity_lift: 1.40,
        }],
        ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
    )
    .expect("overnight boundary should validate");

    assert_eq!(derived_candidates.len(), 1);
    assert_eq!(
        derived_candidates[0].close_timestamp_utc_nanos,
        utc_nanos(2026, 2, 2, 21, 0, 0)
    );
    assert_eq!(
        derived_candidates[0].open_timestamp_utc_nanos,
        utc_nanos(2026, 2, 3, 14, 30, 0)
    );
    assert!((derived_candidates[0].gap_return + 0.013).abs() <= 1e-12);
    assert_eq!(derived_candidates[0].post_shock_activity_lift, 1.40);
}

#[test]
fn derived_close_open_gap_detection_emits_weekend_gap_candidate() {
    let calendar_registry = test_calendar_registry();

    let derived_candidates = derive_close_open_gap_candidates(
        "XNYS",
        &calendar_registry,
        test_source_window(),
        &[ReturnComponentsObservedCloseOpenBoundaryObservation {
            close_timestamp_utc_nanos: utc_nanos(2026, 2, 6, 21, 0, 0),
            close_price: 100.0,
            open_timestamp_utc_nanos: utc_nanos(2026, 2, 9, 14, 30, 0),
            open_price: 100.0 * 0.012_f64.exp(),
            post_shock_activity_lift: 1.15,
        }],
        ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
    )
    .expect("weekend boundary should validate");

    assert_eq!(derived_candidates.len(), 1);
    assert_eq!(
        derived_candidates[0].close_timestamp_utc_nanos,
        utc_nanos(2026, 2, 6, 21, 0, 0)
    );
    assert_eq!(
        derived_candidates[0].open_timestamp_utc_nanos,
        utc_nanos(2026, 2, 9, 14, 30, 0)
    );
    assert!((derived_candidates[0].gap_return - 0.012).abs() <= 1e-12);
    assert_eq!(derived_candidates[0].post_shock_activity_lift, 1.15);
}

#[test]
fn derived_close_open_gap_detection_rejects_non_boundary_timestamps() {
    let calendar_registry = test_calendar_registry();

    let error = derive_close_open_gap_candidates(
        "XNYS",
        &calendar_registry,
        test_source_window(),
        &[ReturnComponentsObservedCloseOpenBoundaryObservation {
            close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 20, 0, 0),
            close_price: 100.0,
            open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
            open_price: 100.0 * (-0.013_f64).exp(),
            post_shock_activity_lift: 1.40,
        }],
        ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
    )
    .expect_err("non-boundary close timestamp must fail");

    assert!(matches!(
        error,
        ReturnComponentsStageCandidateGenerationInputError::InvalidCloseBoundaryTransition { .. }
    ));
}

#[test]
fn nig_search_candidates_remain_non_empty_for_tiny_fx_return_volatility() {
    let summary_statistics = ReturnComponentsStageObservedSignalSummaryStatistics {
        known_calendar_event_count: 3,
        known_realized_sample_count: 1,
        source_window_duration_hours: 12.0,
        observed_open_session_return_count: 2_000,
        continuous_fit_return_count: 1_980,
        intraday_jump_candidate_count: 0,
        intraday_jump_episode_count: 0,
        selected_surprise_jump_episode_count: 0,
        close_open_gap_candidate_count: 0,
        close_open_gap_episode_count: 0,
        open_session_sell_share_proxy: 0.51,
        mean_continuous_business_time_scale: 1.0,
        continuous_return_mean: 0.0,
        continuous_return_volatility: 2.5e-6,
        continuous_return_skewness: -0.15,
        continuous_return_excess_kurtosis: 1.2,
        observed_continuous_log_price_span: 0.0115,
        observed_log_price_span: 0.012,
        mean_abs_known_open_jump_return: 0.0008,
        mean_abs_known_closed_gap_return: 0.0,
        mean_abs_surprise_open_jump_return: 0.0,
        mean_abs_surprise_closed_gap_return: 0.0,
        mean_known_post_shock_activity_lift: 1.05,
        mean_surprise_post_shock_activity_lift: 0.0,
        known_activity_coupling_target: 0.05,
        surprise_activity_coupling_target: 0.0,
        known_news_coverage_ratio: 1.0,
        surprise_arrival_intensity_per_hour: 0.0,
    };

    let sorted_signals = ReturnComponentsStageSortedSignalInputs {
        known_return_samples: Vec::new(),
        observed_open_session_returns: vec![
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                log_return: 1.0e-6,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.5,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                log_return: -1.0e-6,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.5,
            },
        ],
        continuous_fit_returns: vec![
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                log_return: 1.0e-6,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.5,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
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
        source_window: test_source_window(),
    };

    let calibration_sample = continuous_fit::build_continuous_return_calibration_sample(
        &sorted_signals,
        &[1.0, 1.0],
        summary_statistics.open_session_sell_share_proxy,
    );

    let solver_settings =
        CalibrationOptimizerSettings::try_new("adaptive_search", 32, 1, 1.0e-8, 1.0e-8, 1.0)
            .expect("solver settings should validate");

    let nig_candidates =
        continuous_fit::build_nig_search_candidates(&calibration_sample, 17, &solver_settings);

    assert!(!nig_candidates.is_empty());
    assert!(
        nig_candidates
            .iter()
            .all(|candidate| candidate.configuration.delta
                >= return_components_stage_positive_domain_floor())
    );
}

#[test]
fn return_distribution_summary_normalizes_each_return_by_business_time_scale() {
    let summary = summarize_return_distribution(&[0.01, 0.04], &[1.0, 4.0]);

    assert!((summary.mean_return - 0.01).abs() <= 1.0e-12);
    assert!((summary.volatility - 0.007071067811865475).abs() <= 1.0e-12);
    assert!(summary.volatility < sample_standard_deviation(&[0.01, 0.04]));
}

#[test]
fn zero_inflated_open_returns_keep_small_non_zero_moves_in_continuous_fit_sample() {
    let candidate_generation_input = ReturnComponentsStageCandidateGenerationInput {
        known_news_calendar: ReturnComponentsKnownNewsCalendarInput {
            source_window: test_source_window(),
            scheduled_event_timestamps_utc_nanos: Vec::new(),
        },
        realized_known_news_return_samples: Vec::new(),
        observed_open_session_returns: vec![
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                log_return: 0.0,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.5,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                log_return: 0.0,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.5,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 2),
                log_return: 1.0e-4,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.0,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 3),
                log_return: -2.0e-4,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 1.0,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 4),
                log_return: 3.0e-4,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.0,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 5),
                log_return: 5.0e-3,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 1.0,
            },
        ],
        detected_intraday_jump_candidates: vec![ReturnComponentsDetectedIntradayJumpCandidate {
            jump_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 5),
            jump_return: 5.0e-3,
            post_shock_activity_lift: 1.0,
        }],
        observed_close_open_boundary_observations: Vec::new(),
        close_open_gap_detection_policy: ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        open_session_sell_share_proxy: 0.5,
        shared_mark_law_template: SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration {
                shape_k: 2.0,
                scale_theta: 1.0e-4,
            },
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration {
                probability_positive: 0.45,
                eta_plus: 8.0,
                eta_minus: 10.0,
            },
            coupled_gap_marks: CoupledGapMarkConfiguration { sigma_g: 0.001 },
        },
        objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights {
            continuous_distribution_fit_weight: 1.0,
            surprise_arrival_fit_weight: 1.0,
            jump_fit_weight: 1.0,
            gap_fit_weight: 1.0,
            activity_coupling_fit_weight: 1.0,
            price_range_sanity_fit_weight: 0.1,
        },
    };

    let fitted_signal_partition_layer = signal_partition_fit::fit_signal_partition_layer(
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        candidate_generation_input
            .realized_known_news_return_samples
            .as_slice(),
        candidate_generation_input
            .detected_intraday_jump_candidates
            .as_slice(),
    );

    let fitted_surprise_episode_selection_layer =
        surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
            candidate_generation_input
                .detected_intraday_jump_candidates
                .as_slice(),
            fitted_signal_partition_layer.continuous_return_filter_threshold,
        );

    let sorted_signals = sort_candidate_generation_signals(
        &candidate_generation_input,
        &[],
        fitted_signal_partition_layer,
        &fitted_surprise_episode_selection_layer,
    );

    let retained_non_zero_returns: Vec<f64> = sorted_signals
        .continuous_fit_returns
        .iter()
        .map(|observation| observation.log_return)
        .filter(|return_value| *return_value != 0.0)
        .collect();

    assert_eq!(sorted_signals.continuous_fit_returns.len(), 6);
    assert_eq!(
        retained_non_zero_returns,
        vec![1.0e-4, -2.0e-4, 3.0e-4, 5.0e-3]
    );
}

#[test]
fn candidate_generation_preserves_multiple_nig_candidates() {
    let candidate_generation_input = ReturnComponentsStageCandidateGenerationInput {
        known_news_calendar: ReturnComponentsKnownNewsCalendarInput {
            source_window: test_source_window(),
            scheduled_event_timestamps_utc_nanos: Vec::new(),
        },
        realized_known_news_return_samples: Vec::new(),
        observed_open_session_returns: vec![
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                log_return: 1.0e-4,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.20,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                log_return: -0.7e-4,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.80,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 2),
                log_return: 1.5e-4,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.30,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 3),
                log_return: -1.2e-4,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.70,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 4),
                log_return: 0.8e-4,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.35,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 5),
                log_return: -1.4e-4,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.65,
            },
        ],
        detected_intraday_jump_candidates: Vec::new(),
        observed_close_open_boundary_observations: Vec::new(),
        close_open_gap_detection_policy: ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        open_session_sell_share_proxy: 0.5,
        shared_mark_law_template: SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration {
                shape_k: 2.0,
                scale_theta: 1.0e-4,
            },
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration {
                probability_positive: 0.45,
                eta_plus: 8.0,
                eta_minus: 10.0,
            },
            coupled_gap_marks: CoupledGapMarkConfiguration { sigma_g: 0.001 },
        },
        objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights {
            continuous_distribution_fit_weight: 1.0,
            surprise_arrival_fit_weight: 1.0,
            jump_fit_weight: 1.0,
            gap_fit_weight: 1.0,
            activity_coupling_fit_weight: 1.0,
            price_range_sanity_fit_weight: 0.1,
        },
    };
    let solver_settings =
        CalibrationOptimizerSettings::try_new("adaptive_search", 32, 1, 1.0e-8, 1.0e-8, 1.0)
            .expect("solver settings should validate");

    let generated_candidates = generate_return_components_stage_candidate_inputs(
        &candidate_generation_input,
        &[],
        "test_filter",
        None,
        17,
        &solver_settings,
    )
    .expect("candidate generation should succeed");

    assert_eq!(
        generated_candidates.candidate_inputs.len(),
        generated_candidates
            .candidate_family_counts
            .nig_candidate_count
    );
    assert!(generated_candidates.candidate_inputs.len() > 1);
}

#[test]
fn continuous_fit_excludes_exact_known_news_attributed_open_return_timestamps() {
    let candidate_generation_input = ReturnComponentsStageCandidateGenerationInput {
        known_news_calendar: ReturnComponentsKnownNewsCalendarInput {
            source_window: test_source_window(),
            scheduled_event_timestamps_utc_nanos: vec![utc_nanos(2026, 2, 2, 14, 0, 0)],
        },
        realized_known_news_return_samples: vec![ReturnComponentsRealizedKnownNewsReturnSample {
            event_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
            attributed_open_return_timestamp_utc_nanos: Some(utc_nanos(2026, 2, 2, 14, 0, 2)),
            open_window_jump_return: 1.0e-3,
            closed_window_gap_return: 0.0,
            post_shock_activity_lift: 1.2,
        }],
        observed_open_session_returns: vec![
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                log_return: 1.0e-4,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.5,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 2),
                log_return: 2.0e-4,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.5,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 3),
                log_return: 3.0e-4,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.5,
            },
        ],
        detected_intraday_jump_candidates: Vec::new(),
        observed_close_open_boundary_observations: Vec::new(),
        close_open_gap_detection_policy: ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        open_session_sell_share_proxy: 0.5,
        shared_mark_law_template: SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration {
                shape_k: 2.0,
                scale_theta: 1.0e-4,
            },
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration {
                probability_positive: 0.45,
                eta_plus: 8.0,
                eta_minus: 10.0,
            },
            coupled_gap_marks: CoupledGapMarkConfiguration { sigma_g: 0.001 },
        },
        objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights {
            continuous_distribution_fit_weight: 1.0,
            surprise_arrival_fit_weight: 1.0,
            jump_fit_weight: 1.0,
            gap_fit_weight: 1.0,
            activity_coupling_fit_weight: 1.0,
            price_range_sanity_fit_weight: 0.1,
        },
    };

    let fitted_signal_partition_layer = signal_partition_fit::fit_signal_partition_layer(
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        candidate_generation_input
            .realized_known_news_return_samples
            .as_slice(),
        candidate_generation_input
            .detected_intraday_jump_candidates
            .as_slice(),
    );

    let fitted_surprise_episode_selection_layer =
        surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
            candidate_generation_input
                .detected_intraday_jump_candidates
                .as_slice(),
            fitted_signal_partition_layer.continuous_return_filter_threshold,
        );

    let sorted_signals = sort_candidate_generation_signals(
        &candidate_generation_input,
        &[],
        fitted_signal_partition_layer,
        &fitted_surprise_episode_selection_layer,
    );

    let retained_timestamps: Vec<i64> = sorted_signals
        .continuous_fit_returns
        .iter()
        .map(|observation| observation.return_timestamp_utc_nanos)
        .collect();

    assert!(retained_timestamps.contains(&utc_nanos(2026, 2, 2, 14, 0, 1)));
    assert!(!retained_timestamps.contains(&utc_nanos(2026, 2, 2, 14, 0, 2)));
}

#[test]
fn surprise_jump_episode_clustering_merges_adjacent_candidates() {
    let fitted_layer = surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
        &[
            ReturnComponentsDetectedIntradayJumpCandidate {
                jump_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                jump_return: 2.0e-4,
                post_shock_activity_lift: 1.05,
            },
            ReturnComponentsDetectedIntradayJumpCandidate {
                jump_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                jump_return: -1.0e-4,
                post_shock_activity_lift: 1.25,
            },
            ReturnComponentsDetectedIntradayJumpCandidate {
                jump_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 15, 0),
                jump_return: 5.0e-4,
                post_shock_activity_lift: 1.10,
            },
        ],
        1.0e-4,
    );

    let episodes = fitted_layer.surprise_jump_episodes;

    assert_eq!(episodes.len(), 2);
    assert!((episodes[0].cumulative_jump_return - 1.0e-4).abs() <= 1.0e-12);
    assert_eq!(episodes[0].post_shock_activity_lift, 1.25);
    assert!((episodes[1].cumulative_jump_return - 5.0e-4).abs() <= 1.0e-12);
}

#[test]
fn exact_candidate_evaluation_matches_stage_bucket_construction() {
    let candidate_generation_input = ReturnComponentsStageCandidateGenerationInput {
        known_news_calendar: ReturnComponentsKnownNewsCalendarInput {
            source_window: test_source_window(),
            scheduled_event_timestamps_utc_nanos: Vec::new(),
        },
        realized_known_news_return_samples: Vec::new(),
        observed_open_session_returns: vec![
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                log_return: 1.0e-6,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.4,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                log_return: -2.0e-6,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.6,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 2),
                log_return: 1.5e-6,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.5,
            },
        ],
        detected_intraday_jump_candidates: Vec::new(),
        observed_close_open_boundary_observations: Vec::new(),
        close_open_gap_detection_policy: ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        open_session_sell_share_proxy: 0.5,
        shared_mark_law_template: SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration {
                shape_k: 2.0,
                scale_theta: 1.0e-4,
            },
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration {
                probability_positive: 0.45,
                eta_plus: 8.0,
                eta_minus: 10.0,
            },
            coupled_gap_marks: CoupledGapMarkConfiguration { sigma_g: 0.001 },
        },
        objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights {
            continuous_distribution_fit_weight: 1.0,
            surprise_arrival_fit_weight: 1.0,
            jump_fit_weight: 1.0,
            gap_fit_weight: 1.0,
            activity_coupling_fit_weight: 1.0,
            price_range_sanity_fit_weight: 0.1,
        },
    };

    let fitted_parameters = ReturnComponentsStageFittedParameters {
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration {
            alpha: 100.0,
            beta_0: 0.0,
            delta: 1.0e-6,
            mu: 0.0,
            delta_beta: -1.0,
        },
        news_process_configuration: NewsProcessConfiguration::try_new(
            NewsArrivalConfiguration {
                known: KnownNewsArrivalConfiguration {
                    scheduled_event_timestamps_utc_nanos: Vec::new(),
                },
                surprise: SurpriseNewsArrivalConfiguration {
                    baseline_intensity: 0.0,
                },
            },
            candidate_generation_input.shared_mark_law_template,
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 0.0,
                return_jump_scale: 1.0,
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 0.0,
                return_jump_scale: 1.0,
            },
        )
        .expect("news process configuration should validate"),
        surprise_deterministic_intensity_profile: default_surprise_deterministic_intensity_profile(
        ),
        mean_reversion_fitted_parameters:
            MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
                .expect("zero-gain mean reversion should validate"),
    };

    let fitted_signal_partition_layer = signal_partition_fit::fit_signal_partition_layer(
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        candidate_generation_input
            .realized_known_news_return_samples
            .as_slice(),
        candidate_generation_input
            .detected_intraday_jump_candidates
            .as_slice(),
    );

    let fitted_surprise_episode_selection_layer =
        surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
            candidate_generation_input
                .detected_intraday_jump_candidates
                .as_slice(),
            fitted_signal_partition_layer.continuous_return_filter_threshold,
        );

    let sorted_signals = sort_candidate_generation_signals(
        &candidate_generation_input,
        &[],
        fitted_signal_partition_layer,
        &fitted_surprise_episode_selection_layer,
    );

    let (summary_statistics, calibration_sample) =
        summarize_candidate_generation_signals(&candidate_generation_input, &sorted_signals, None);

    let exact_evaluation = evaluate_return_components_exact_candidate(
        candidate_generation_input.clone(),
        None,
        summary_statistics,
        fitted_parameters.clone(),
        17,
    )
    .expect("exact evaluation should succeed");

    let continuous_evaluation = continuous_fit::evaluate_continuous_nig_candidate(
        fitted_parameters.nig_shared_plus_shift_configuration,
        &calibration_sample,
        summary_statistics,
        17,
    );

    let surprise_arrival_family = ReturnComponentsSurpriseNewsArrivalCandidateFamily {
        baseline_intensity: fitted_parameters
            .news_process_configuration
            .arrivals
            .surprise
            .baseline_intensity,
        weekly_hour_utc_multipliers: fitted_parameters
            .surprise_deterministic_intensity_profile
            .weekly_hour_utc_multipliers
            .clone(),
        profile_resolution_hours: fitted_parameters
            .surprise_deterministic_intensity_profile
            .profile_resolution_hours,
    };

    let return_jump_family = ReturnComponentsReturnJumpMarkScaleCandidateFamily {
        sigma_g: fitted_parameters
            .news_process_configuration
            .shared_mark_law
            .coupled_gap_marks
            .sigma_g,
    };

    let activity_coupling_family = ReturnComponentsActivityCouplingCandidateFamily {
        known_activity_coupling_scale: fitted_parameters
            .news_process_configuration
            .known_component_mark_scales
            .activity_coupling_scale,
        surprise_activity_coupling_scale: fitted_parameters
            .news_process_configuration
            .surprise_component_mark_scales
            .activity_coupling_scale,
    };

    let fitted_diagnostic_policy = diagnostic_policy_fit::fit_diagnostic_policy();
    let fitted_objective_scoring_policy =
        objective_scoring_policy_fit::fit_objective_scoring_policy(summary_statistics);

    let expected_jump_diagnostic = build_jump_diagnostic(
        &fitted_diagnostic_policy,
        summary_statistics,
        &return_jump_family,
        candidate_generation_input.shared_mark_law_template,
    );
    let expected_surprise_arrival_diagnostic = build_surprise_arrival_diagnostic(
        &fitted_diagnostic_policy,
        summary_statistics,
        &surprise_arrival_family,
        &fitted_parameters.surprise_deterministic_intensity_profile,
    );
    let expected_gap_diagnostic = build_gap_diagnostic(
        &fitted_diagnostic_policy,
        summary_statistics,
        &return_jump_family,
        candidate_generation_input.shared_mark_law_template,
    );
    let expected_activity_coupling_diagnostic = build_activity_coupling_diagnostic(
        &fitted_diagnostic_policy,
        summary_statistics,
        &activity_coupling_family,
    );

    let fitted_price_range_sanity_policy =
        price_range_policy_fit::fit_price_range_sanity_policy(summary_statistics);

    let expected_price_range_sanity_diagnostic = build_price_range_sanity_diagnostic(
        &fitted_diagnostic_policy,
        summary_statistics,
        continuous_evaluation.diagnostic,
        &surprise_arrival_family,
        &return_jump_family,
        candidate_generation_input.shared_mark_law_template,
        &fitted_price_range_sanity_policy,
    );

    let expected_buckets = objective_scoring_policy_fit::score_candidate_metric_buckets(
        &fitted_objective_scoring_policy,
        continuous_evaluation.objective_score,
        &expected_surprise_arrival_diagnostic,
        &expected_jump_diagnostic,
        &expected_gap_diagnostic,
        &expected_activity_coupling_diagnostic,
        &expected_price_range_sanity_diagnostic,
    );

    assert_eq!(exact_evaluation.objective_metric_buckets, expected_buckets);
    assert_eq!(
        exact_evaluation.return_distribution_diagnostic,
        continuous_evaluation.diagnostic
    );
    assert_eq!(
        exact_evaluation.surprise_arrival_diagnostic,
        expected_surprise_arrival_diagnostic
    );
    assert_eq!(exact_evaluation.jump_diagnostic, expected_jump_diagnostic);
    assert_eq!(exact_evaluation.gap_diagnostic, expected_gap_diagnostic);
    assert_eq!(
        exact_evaluation.activity_coupling_diagnostic,
        expected_activity_coupling_diagnostic
    );
    assert_eq!(
        exact_evaluation.price_range_sanity_diagnostic,
        expected_price_range_sanity_diagnostic
    );
}

fn test_source_window() -> ReturnComponentsStageSignalSourceWindow {
    ReturnComponentsStageSignalSourceWindow {
        window_start_utc_nanos: utc_nanos(2026, 2, 2, 13, 0, 0),
        window_end_utc_nanos: utc_nanos(2026, 2, 10, 0, 0, 0),
    }
}

fn test_calendar_registry() -> ExchangeCalendarRegistry {
    ExchangeCalendarRegistryLoader::load_from_json(
        r#"
{
  "schema_version": "calendar.registry.v1",
  "artifact_date": "2026-01-31",
  "exchanges": [
    {
      "schema_version": "calendar.v1",
      "exchange_code": "XNYS",
      "exchange_name": "New York Stock Exchange",
      "timezone_name": "America/New_York",
      "rule_metadata": {
        "holiday_calendar_code": "US_EQUITIES",
        "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
      },
      "regular_sessions": [
        {"weekday": "Monday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Tuesday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Wednesday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Thursday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Friday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]}
      ],
      "holiday_dates": [],
      "early_close_sessions": []
    }
  ]
}
"#,
    )
    .expect("calendar registry fixture should load")
}
