use std::cmp::Ordering;

use super::super::objective::{
    business_time_bucket_return_scale_fit, compare_joint_candidate_breakdowns,
    intraday_return_level_horizon_fit, joint_primary_metrics_from_breakdown,
    return_asymmetry_horizon_fit, root_mean_square,
};
use super::super::setup::{
    build_restart_seeds, project_basin_diagnostic_into_joint_nig_coordinates,
};
use super::{
    JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT, JointRefinementActiveParameterLayout,
    JointRefinementAdmissibilityThresholds, JointRefinementCoarseConsistencySummary,
    JointRefinementSeedSummary, admissibility_threshold_from_sample_count,
    desired_joint_refinement_restart_count, extend_joint_coarse_tick_consistency_metrics_for_seed,
    joint_breakdown_is_admissible, joint_refinement_base_schedule_components,
    joint_refinement_budget_schedule, maximum_parallel_restart_count,
    sampled_observed_tick_returns,
};
use crate::stages::joint_metrics::{
    JointRefinementAggregatedIntervalObservation,
    JointRefinementBusinessTimeBucketReturnScaleSummaries, JointRefinementReturnScaleSummary,
    JointRefinementRollingVarianceActivityMetric,
};
use crate::stages::return_components::{
    ReturnComponentsObservedOpenSessionReturn,
    ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
    ReturnComponentsStageContinuousReturnCalibrationSample,
    build_continuous_return_calibration_sample_from_components,
};
use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, JointRefinementStageObjectiveBreakdown,
};

fn build_test_solver_settings(cpu_count: usize) -> CalibrationOptimizerSettings {
    CalibrationOptimizerSettings::try_new("joint-test", 256, cpu_count, 1.0e-6, 1.0e-6, 10.0)
        .expect("test optimizer settings should be valid")
}

fn build_test_active_parameter_layout() -> JointRefinementActiveParameterLayout {
    JointRefinementActiveParameterLayout {
        hawkes_block: vec![0, 1, 2, 3],
        nig_block: vec![4, 5, 6, 7],
        coupling_block: vec![8, 9, 10],
        log_lambda_scale_index: 0,
        log_beta_scale_index: 1,
        log_self_excitation_scale_index: 2,
        log_cross_excitation_scale_index: 3,
        known_activity_scale_index: Some(8),
        surprise_activity_scale_index: Some(9),
        surprise_baseline_scale_index: Some(10),
    }
}

fn build_test_basin_diagnostic(
    family_label: &str,
    final_log_alpha: f64,
    final_buy_skew_coordinate: f64,
    final_sell_relative_coordinate: f64,
    final_log_delta_offset: f64,
) -> ReturnComponentsStageContinuousOptimizerBasinDiagnostic {
    ReturnComponentsStageContinuousOptimizerBasinDiagnostic {
        family_label: family_label.to_string(),
        seed_objective: 1.0,
        local_objective: 0.9,
        final_objective: 0.8,
        local_evaluation_count: 8,
        stop_reason: "test".to_string(),
        seed_log_alpha: final_log_alpha,
        seed_buy_skew_coordinate: final_buy_skew_coordinate,
        seed_sell_relative_coordinate: final_sell_relative_coordinate,
        seed_log_delta_offset: final_log_delta_offset,
        final_log_alpha,
        final_buy_skew_coordinate,
        final_sell_relative_coordinate,
        final_log_delta_offset,
        final_alpha: final_log_alpha.exp(),
        final_beta_0: -0.5,
        final_delta: final_log_delta_offset.exp(),
        final_delta_beta: -0.2,
        profile_fit_score: 0.8,
        fitted_return_volatility: 0.01,
        fitted_skewness: -0.1,
    }
}

fn build_test_calibration_sample() -> ReturnComponentsStageContinuousReturnCalibrationSample {
    build_continuous_return_calibration_sample_from_components(
        vec![0.012, -0.009, 0.006, -0.004, 0.011, -0.007],
        vec![1.0, 0.8, 1.2, 0.9, 1.1, 0.95],
        vec![0.48, 0.53, 0.51, 0.47, 0.55, 0.49],
        0.5,
    )
}

fn build_test_joint_breakdown(
    tick_return_asymmetry_fit: f64,
    tick_return_scale_fit: f64,
    weighted_objective: f64,
) -> JointRefinementStageObjectiveBreakdown {
    JointRefinementStageObjectiveBreakdown {
        aggregated_likelihood_fit: 1.0,
        aggregated_return_shape_fit: 1.0,
        return_asymmetry_fit: 1.0,
        tick_return_asymmetry_fit,
        aggregated_return_scale_fit: 1.0,
        tick_return_scale_fit,
        business_time_return_scale_fit: 1.0,
        intraday_return_level_fit: 1.0,
        activity_shape_fit: 1.0,
        activity_level_fit: 1.0,
        rv_activity_coupling_fit: 1.0,
        price_span_sanity_fit: 0.5,
        option_spread_normalized_price_fit: 0.0,
        weighted_objective,
        hard_constraint_penalty: 0.0,
    }
}

fn build_test_admissibility_thresholds() -> JointRefinementAdmissibilityThresholds {
    JointRefinementAdmissibilityThresholds {
        aggregated_likelihood_fit: 2.0,
        aggregated_return_shape_fit: 2.0,
        return_asymmetry_fit: 2.0,
        aggregated_return_scale_fit: 2.0,
        business_time_return_scale_fit: 2.0,
        intraday_return_level_fit: 2.0,
        activity_shape_fit: 2.0,
        activity_level_fit: 2.0,
        rv_activity_coupling_fit: 2.0,
        price_span_sanity_fit: 2.0,
    }
}

fn build_test_interval_observation(
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
    log_return: f64,
    tick_count: usize,
    sell_share_proxy: f64,
) -> JointRefinementAggregatedIntervalObservation {
    JointRefinementAggregatedIntervalObservation {
        interval_start_utc_nanos,
        interval_end_utc_nanos,
        log_return,
        tick_count,
        sell_share_proxy,
    }
}

fn build_test_seed_summary(
    simulated_5m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_15m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_5m_return_scale_summary: JointRefinementReturnScaleSummary,
    simulated_15m_return_scale_summary: JointRefinementReturnScaleSummary,
) -> JointRefinementSeedSummary {
    JointRefinementSeedSummary {
        simulated_5m_intervals,
        simulated_15m_intervals,
        simulated_tick_shape_returns: vec![0.01, -0.01],
        simulated_5m_shape_returns: vec![0.02, -0.02],
        simulated_15m_shape_returns: vec![0.03, -0.03],
        simulated_tick_return_scale_summary: JointRefinementReturnScaleSummary {
            mean_absolute_return: 0.01,
            realized_variance: 0.0001,
            absolute_return_quantile_q90: 0.02,
        },
        simulated_5m_return_scale_summary,
        simulated_15m_return_scale_summary,
        simulated_5m_business_time_return_scale_summaries:
            JointRefinementBusinessTimeBucketReturnScaleSummaries {
                bucket_summaries: [None; 4],
            },
        simulated_15m_business_time_return_scale_summaries:
            JointRefinementBusinessTimeBucketReturnScaleSummaries {
                bucket_summaries: [None; 4],
            },
        simulated_tick_intraday_mean_absolute_returns: vec![0.01; 24],
        simulated_5m_intraday_mean_absolute_returns: vec![0.02; 24],
        simulated_15m_intraday_mean_absolute_returns: vec![0.03; 24],
        simulated_hourly_tick_counts: vec![1.0; 24],
        simulated_15m_rv_activity_metric: JointRefinementRollingVarianceActivityMetric {
            correlation: 0.0,
            standardized_path_distance: 0.0,
        },
    }
}

#[test]
fn joint_refinement_desired_restart_count_is_cpu_count_invariant_and_budget_derived() {
    let single_core_restart_count =
        desired_joint_refinement_restart_count(&build_test_solver_settings(1));
    let eight_core_restart_count =
        desired_joint_refinement_restart_count(&build_test_solver_settings(8));
    let high_core_solver_settings = build_test_solver_settings(64);
    let high_core_restart_count =
        desired_joint_refinement_restart_count(&high_core_solver_settings);

    assert_eq!(
        single_core_restart_count,
        maximum_parallel_restart_count(&build_test_solver_settings(1))
    );
    assert_eq!(eight_core_restart_count, single_core_restart_count);
    assert_eq!(
        high_core_restart_count,
        maximum_parallel_restart_count(&high_core_solver_settings)
    );
    assert!(
        high_core_restart_count > JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT,
        "restart frontier should expand beyond the block count when budget allows",
    );
}

#[test]
fn joint_refinement_high_core_budget_stays_within_base_block_budget() {
    let solver_settings = build_test_solver_settings(64);
    let (base_outer_cycle_count, base_powell_settings) =
        joint_refinement_base_schedule_components(&solver_settings);
    let base_total_block_evaluation_budget = JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT
        * base_outer_cycle_count
        * base_powell_settings.evaluation_budget;

    let high_core_schedule = joint_refinement_budget_schedule(
        &solver_settings,
        desired_joint_refinement_restart_count(&solver_settings),
    );
    let scheduled_block_evaluation_budget = high_core_schedule.restart_count
        * high_core_schedule.outer_cycle_count
        * JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT
        * high_core_schedule.powell_settings.evaluation_budget;

    assert!(
        scheduled_block_evaluation_budget <= base_total_block_evaluation_budget,
        "scheduled block evaluation budget exceeded base budget: scheduled={scheduled_block_evaluation_budget} base={base_total_block_evaluation_budget}",
    );
}

#[test]
fn joint_refinement_restart_seed_generation_is_deterministic() {
    let active_parameter_layout = build_test_active_parameter_layout();
    let baseline_parameter_vector = vec![0.0; active_parameter_layout.vector_length()];
    let baseline_15m_calibration_sample = build_test_calibration_sample();

    let lower_bounds = vec![-1.0; baseline_parameter_vector.len()];
    let upper_bounds = vec![2.0; baseline_parameter_vector.len()];

    let first_restart_seeds = build_restart_seeds(
        baseline_parameter_vector.as_slice(),
        &active_parameter_layout,
        lower_bounds.as_slice(),
        upper_bounds.as_slice(),
        &baseline_15m_calibration_sample,
        &[],
        0.25,
        8,
    );
    let second_restart_seeds = build_restart_seeds(
        baseline_parameter_vector.as_slice(),
        &active_parameter_layout,
        lower_bounds.as_slice(),
        upper_bounds.as_slice(),
        &baseline_15m_calibration_sample,
        &[],
        0.25,
        8,
    );

    assert_eq!(first_restart_seeds, second_restart_seeds);
    assert_eq!(first_restart_seeds.len(), 8);
    assert_eq!(first_restart_seeds[0].0, "baseline");
    assert_eq!(first_restart_seeds[1].0, "activity_aligned");
    assert_eq!(first_restart_seeds[2].0, "midpoint");
    assert_eq!(first_restart_seeds[3].0, "halton_01");
}

#[test]
fn joint_refinement_restart_seeds_include_return_component_nig_basins() {
    let active_parameter_layout = build_test_active_parameter_layout();
    let baseline_parameter_vector = vec![0.0; active_parameter_layout.vector_length()];
    let baseline_15m_calibration_sample = build_test_calibration_sample();
    let lower_bounds = vec![-1.0; baseline_parameter_vector.len()];
    let upper_bounds = vec![2.0; baseline_parameter_vector.len()];
    let basin_diagnostics = vec![build_test_basin_diagnostic(
        "skew_heavy",
        0.4,
        -0.2,
        1.3,
        -0.6,
    )];

    let restart_seeds = build_restart_seeds(
        baseline_parameter_vector.as_slice(),
        &active_parameter_layout,
        lower_bounds.as_slice(),
        upper_bounds.as_slice(),
        &baseline_15m_calibration_sample,
        basin_diagnostics.as_slice(),
        0.25,
        8,
    );
    let projected_search_point = project_basin_diagnostic_into_joint_nig_coordinates(
        &baseline_15m_calibration_sample,
        &basin_diagnostics[0],
    )
    .expect("basin diagnostic should project into joint coordinates");
    let clamped_projected_coordinates = [
        projected_search_point.log_alpha.clamp(
            lower_bounds[active_parameter_layout.nig_block[0]],
            upper_bounds[active_parameter_layout.nig_block[0]],
        ),
        projected_search_point.buy_skew_coordinate.clamp(
            lower_bounds[active_parameter_layout.nig_block[1]],
            upper_bounds[active_parameter_layout.nig_block[1]],
        ),
        projected_search_point.sell_relative_coordinate.clamp(
            lower_bounds[active_parameter_layout.nig_block[2]],
            upper_bounds[active_parameter_layout.nig_block[2]],
        ),
        projected_search_point.log_delta_offset.clamp(
            lower_bounds[active_parameter_layout.nig_block[3]],
            upper_bounds[active_parameter_layout.nig_block[3]],
        ),
    ];

    assert_eq!(restart_seeds[3].0, "nig_basin_skew_heavy");
    assert_eq!(restart_seeds[4].0, "nig_basin_skew_heavy_activity_aligned");
    assert_eq!(
        restart_seeds[3].1[active_parameter_layout.nig_block[0]],
        clamped_projected_coordinates[0]
    );
    assert_eq!(
        restart_seeds[3].1[active_parameter_layout.nig_block[1]],
        clamped_projected_coordinates[1]
    );
    assert_eq!(
        restart_seeds[3].1[active_parameter_layout.nig_block[2]],
        clamped_projected_coordinates[2]
    );
    assert_eq!(
        restart_seeds[3].1[active_parameter_layout.nig_block[3]],
        clamped_projected_coordinates[3]
    );
    assert_eq!(
        restart_seeds[4].1[active_parameter_layout.log_lambda_scale_index],
        0.25
    );
    assert_eq!(restart_seeds[5].0, "nig_alpha_delta_skew_heavy");
    assert_eq!(
        restart_seeds[5].1[active_parameter_layout.nig_block[0]],
        clamped_projected_coordinates[0]
    );
    assert_eq!(
        restart_seeds[5].1[active_parameter_layout.nig_block[1]],
        baseline_parameter_vector[active_parameter_layout.nig_block[1]]
    );
    assert_eq!(
        restart_seeds[5].1[active_parameter_layout.nig_block[2]],
        baseline_parameter_vector[active_parameter_layout.nig_block[2]]
    );
    assert_eq!(
        restart_seeds[5].1[active_parameter_layout.nig_block[3]],
        clamped_projected_coordinates[3]
    );
    assert_eq!(
        restart_seeds[6].0,
        "nig_alpha_delta_skew_heavy_activity_aligned"
    );
    assert_eq!(
        restart_seeds[6].1[active_parameter_layout.log_lambda_scale_index],
        0.25
    );
}

#[test]
fn business_time_bucket_return_scale_fit_penalizes_oversized_returns_within_same_regimes() {
    let observed_summaries = JointRefinementBusinessTimeBucketReturnScaleSummaries {
        bucket_summaries: [Some(JointRefinementReturnScaleSummary {
            mean_absolute_return: 0.001,
            realized_variance: 1.0e-6,
            absolute_return_quantile_q90: 0.002,
        }); 4],
    };
    let matched_summaries = observed_summaries;
    let amplified_summaries = JointRefinementBusinessTimeBucketReturnScaleSummaries {
        bucket_summaries: [Some(JointRefinementReturnScaleSummary {
            mean_absolute_return: 0.004,
            realized_variance: 1.6e-5,
            absolute_return_quantile_q90: 0.008,
        }); 4],
    };

    let matched_fit = business_time_bucket_return_scale_fit(matched_summaries, observed_summaries);
    let amplified_fit =
        business_time_bucket_return_scale_fit(amplified_summaries, observed_summaries);

    assert_eq!(matched_fit, 0.0);
    assert!(amplified_fit > matched_fit);
}

#[test]
fn business_time_bucket_return_scale_fit_penalizes_missing_supported_buckets() {
    let observed_summaries = JointRefinementBusinessTimeBucketReturnScaleSummaries {
        bucket_summaries: [Some(JointRefinementReturnScaleSummary {
            mean_absolute_return: 0.001,
            realized_variance: 1.0e-6,
            absolute_return_quantile_q90: 0.002,
        }); 4],
    };
    let candidate_summaries = JointRefinementBusinessTimeBucketReturnScaleSummaries {
        bucket_summaries: [
            None,
            Some(JointRefinementReturnScaleSummary {
                mean_absolute_return: 0.001,
                realized_variance: 1.0e-6,
                absolute_return_quantile_q90: 0.002,
            }),
            Some(JointRefinementReturnScaleSummary {
                mean_absolute_return: 0.001,
                realized_variance: 1.0e-6,
                absolute_return_quantile_q90: 0.002,
            }),
            Some(JointRefinementReturnScaleSummary {
                mean_absolute_return: 0.001,
                realized_variance: 1.0e-6,
                absolute_return_quantile_q90: 0.002,
            }),
        ],
    };

    let matched_fit = business_time_bucket_return_scale_fit(observed_summaries, observed_summaries);
    let missing_bucket_fit =
        business_time_bucket_return_scale_fit(candidate_summaries, observed_summaries);

    assert_eq!(matched_fit, 0.0);
    assert!(missing_bucket_fit > matched_fit);
}

#[test]
fn admissibility_threshold_tightens_with_sample_count() {
    let sparse_threshold = admissibility_threshold_from_sample_count(1);
    let dense_threshold = admissibility_threshold_from_sample_count(400);

    assert_eq!(sparse_threshold, 2.0);
    assert!(dense_threshold < sparse_threshold);
    assert!(dense_threshold > 1.0);
}

#[test]
fn intraday_return_level_horizon_fit_penalizes_uniformly_oversized_hourly_profiles() {
    let observed_profile = vec![0.001; 24];
    let matched_profile = observed_profile.clone();
    let amplified_profile = vec![0.004; 24];

    let matched_fit =
        intraday_return_level_horizon_fit(matched_profile.as_slice(), observed_profile.as_slice());
    let amplified_fit = intraday_return_level_horizon_fit(
        amplified_profile.as_slice(),
        observed_profile.as_slice(),
    );

    assert_eq!(matched_fit, 0.0);
    assert!(amplified_fit > matched_fit);
}

#[test]
fn return_asymmetry_horizon_fit_penalizes_mirrored_asymmetry() {
    let observed_profile = vec![-0.004, -0.002, -0.001, 0.0, 0.001, 0.002];
    let matched_profile = observed_profile.clone();
    let mirrored_profile = observed_profile
        .iter()
        .map(|value| -*value)
        .collect::<Vec<_>>();

    let matched_fit =
        return_asymmetry_horizon_fit(matched_profile.as_slice(), observed_profile.as_slice());
    let mirrored_fit =
        return_asymmetry_horizon_fit(mirrored_profile.as_slice(), observed_profile.as_slice());

    assert_eq!(matched_fit, 0.0);
    assert!(mirrored_fit > matched_fit);
}

#[test]
fn root_mean_square_penalizes_concentrated_joint_misses() {
    let concentrated_miss = root_mean_square([3.0, 0.0, 0.0].as_slice());
    let balanced_miss = root_mean_square([1.0, 1.0, 1.0].as_slice());

    assert!(concentrated_miss > balanced_miss);
}

#[test]
fn sampled_observed_tick_returns_use_raw_open_session_returns() {
    let observed_open_session_returns = vec![
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 1,
            log_return: 0.01,
            post_shock_activity_lift: 0.0,
            sell_share_proxy: 0.5,
        },
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 2,
            log_return: 0.02,
            post_shock_activity_lift: 0.0,
            sell_share_proxy: 0.5,
        },
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 3,
            log_return: 0.03,
            post_shock_activity_lift: 0.0,
            sell_share_proxy: 0.5,
        },
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 4,
            log_return: 0.04,
            post_shock_activity_lift: 0.0,
            sell_share_proxy: 0.5,
        },
    ];

    let sampled_returns =
        sampled_observed_tick_returns(observed_open_session_returns.as_slice(), 3);

    assert_eq!(sampled_returns, vec![0.01, 0.02, 0.04]);
}

#[test]
fn joint_primary_metrics_include_direct_tick_distribution_terms() {
    let breakdown = build_test_joint_breakdown(1.7, 1.9, 1.2);

    let primary_metrics = joint_primary_metrics_from_breakdown(&breakdown);

    assert_eq!(primary_metrics.len(), 12);
    assert_eq!(primary_metrics[3], 1.7);
    assert_eq!(primary_metrics[5], 1.9);
}

#[test]
fn admissible_candidate_ordering_uses_direct_tick_terms_before_weighted_tie_break() {
    let better_tick_fit = build_test_joint_breakdown(0.8, 0.7, 1.4);
    let worse_tick_fit = build_test_joint_breakdown(1.4, 1.5, 1.1);

    let ordering = compare_joint_candidate_breakdowns(
        1,
        &better_tick_fit,
        true,
        "better_tick_fit",
        1,
        &worse_tick_fit,
        true,
        "worse_tick_fit",
        true,
    );

    assert_eq!(ordering, Ordering::Less);
}

#[test]
fn admissible_candidate_ordering_uses_weighted_objective_after_primary_metrics_tie() {
    let lower_objective = build_test_joint_breakdown(1.0, 1.0, 0.9);
    let higher_objective = build_test_joint_breakdown(1.0, 1.0, 1.1);

    let ordering = compare_joint_candidate_breakdowns(
        1,
        &lower_objective,
        true,
        "lower_objective",
        1,
        &higher_objective,
        true,
        "higher_objective",
        true,
    );

    assert_eq!(ordering, Ordering::Less);
}

#[test]
fn admissible_candidate_ordering_rejects_inadmissible_restart_when_available() {
    let admissible_baseline = build_test_joint_breakdown(1.0, 1.0, 1.2);
    let inadmissible_restart = build_test_joint_breakdown(0.8, 0.8, 0.7);

    let ordering = compare_joint_candidate_breakdowns(
        1,
        &admissible_baseline,
        true,
        "baseline",
        1,
        &inadmissible_restart,
        false,
        "restart_refined",
        true,
    );

    assert_eq!(ordering, Ordering::Less);
}

#[test]
fn coarse_consistency_admissibility_is_baseline_relative() {
    let breakdown = build_test_joint_breakdown(1.0, 1.0, 1.0);
    let metric_count = 24;
    let admissibility_threshold = admissibility_threshold_from_sample_count(metric_count);

    let admissible_summary = JointRefinementCoarseConsistencySummary {
        raw_fit: 1.0,
        normalized_fit: admissibility_threshold,
        admissibility_threshold,
    };
    let inadmissible_summary = JointRefinementCoarseConsistencySummary {
        raw_fit: 1.0,
        normalized_fit: admissibility_threshold + 1.0e-6,
        admissibility_threshold,
    };

    assert!(joint_breakdown_is_admissible(
        &breakdown,
        build_test_admissibility_thresholds(),
        Some(&admissible_summary),
        true,
    ));
    assert!(!joint_breakdown_is_admissible(
        &breakdown,
        build_test_admissibility_thresholds(),
        Some(&inadmissible_summary),
        true,
    ));
    assert!(!joint_breakdown_is_admissible(
        &breakdown,
        build_test_admissibility_thresholds(),
        None,
        true,
    ));
}

#[test]
fn coarse_tick_consistency_prefers_direct_fifteen_minute_coarse_path() {
    let tick_summary = build_test_seed_summary(
        vec![
            build_test_interval_observation(0, 300, 0.01, 2, 0.5),
            build_test_interval_observation(300, 600, -0.02, 2, 0.5),
            build_test_interval_observation(600, 900, 0.03, 2, 0.5),
        ],
        vec![build_test_interval_observation(0, 900, 0.02, 7, 0.5)],
        JointRefinementReturnScaleSummary {
            mean_absolute_return: 0.02,
            realized_variance: 0.0004,
            absolute_return_quantile_q90: 0.03,
        },
        JointRefinementReturnScaleSummary {
            mean_absolute_return: 0.07,
            realized_variance: 0.0049,
            absolute_return_quantile_q90: 0.08,
        },
    );
    let coarse_5m_intervals = vec![
        build_test_interval_observation(0, 300, 0.04, 2, 0.5),
        build_test_interval_observation(300, 600, 0.04, 2, 0.5),
        build_test_interval_observation(600, 900, 0.04, 2, 0.5),
    ];
    let coarse_15m_intervals = vec![build_test_interval_observation(0, 900, 0.25, 11, 0.5)];

    let mut metrics = Vec::new();
    extend_joint_coarse_tick_consistency_metrics_for_seed(
        &mut metrics,
        17,
        Some(coarse_5m_intervals.as_slice()),
        Some(coarse_15m_intervals.as_slice()),
        &tick_summary,
    );

    let direct_fifteen_minute_event_count_metric = metrics
        .iter()
        .find(|metric| metric.horizon_label == "15m" && metric.metric_id == "event_count")
        .expect("15m event-count metric should be present");

    assert_eq!(direct_fifteen_minute_event_count_metric.coarse_value, 11.0);
    assert_eq!(direct_fifteen_minute_event_count_metric.tick_value, 7.0);
}
