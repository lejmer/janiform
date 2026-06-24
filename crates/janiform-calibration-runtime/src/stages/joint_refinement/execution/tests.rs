use std::cmp::Ordering;

use chrono::{DateTime, Utc};
use janiform_model_kernels::return_clock::SideSourceActivityDecomposition;

use super::super::objective::{
    JointCandidateBreakdownComparisonInput, business_time_bucket_return_scale_fit,
    compare_joint_candidate_breakdowns, intraday_return_level_horizon_fit,
    joint_primary_metrics_from_breakdown, return_asymmetry_horizon_fit, root_mean_square,
};
use super::super::setup::{
    ParameterBoundsBuildInput, RestartSeedsWithHaltonAcceptanceBuildInput,
    build_active_parameter_layout, build_baseline_parameter_vector, build_parameter_bounds,
    build_restart_seeds_with_halton_acceptance, parameter_vector_to_nig_search_point,
    project_basin_diagnostic_into_joint_nig_coordinates,
    restart_candidate_screening_max_parallelism_for_test,
};
use super::{
    JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND, JointRefinementActiveParameterLayout,
    JointRefinementCandidateEvaluation, JointRefinementObservedData,
    JointRefinementRawAdmissibilityMetrics, JointRefinementRestartOutcome,
    JointRefinementSeedSummary, admissibility_threshold_from_sample_count,
    desired_joint_refinement_restart_candidate_budget, ensure_valid_selected_restart_outcome,
    extend_joint_coarse_tick_consistency_metrics_for_seed, joint_refinement_budget_schedule,
    option_refinement_candidate_frontier_from_selected_outcomes, sampled_observed_tick_returns,
    test_joint_fitted_parameters, tick_shape_sample_target_count,
};
use crate::stages::joint_metrics::{
    JointRefinementAggregatedIntervalObservation,
    JointRefinementBusinessTimeBucketReturnScaleSummaries,
    JointRefinementBusinessTimeBucketThresholds, JointRefinementReturnScaleSummary,
    JointRefinementRollingVarianceActivityMetric,
};
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageObjectiveTermDiagnostic,
};
use crate::stages::long_variance::metrics::{
    LongVarianceHorizonInputIntervals, long_variance_horizon_summaries,
};
use crate::stages::return_components::{
    ContinuousLikelihoodEvaluatorCounts, NigShapeSearchPoint,
    ReturnComponentsObservedOpenSessionReturn,
    ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
    ReturnComponentsStageContinuousReturnCalibrationSample,
    build_continuous_return_calibration_sample_from_components,
    build_continuous_return_optimizer_context, build_weighted_continuous_return_optimizer_context,
    nig_configuration_from_search_point,
    nig_search_bounds_for_calibration_sample_with_solver_settings,
    nig_search_point_from_configuration,
};
use janiform_calibration_artifacts::{
    CalibrationLossAdmissibility, CalibrationLossFamilyDiagnostic, CalibrationLossTermDiagnostic,
    CalibrationLossTermKind, CalibrationOptimizerSettings, JointRefinementStageObjectiveBreakdown,
    SurpriseNewsDeterministicIntensityProfile,
};

fn build_test_solver_settings(cpu_count: usize) -> CalibrationOptimizerSettings {
    CalibrationOptimizerSettings::try_new("joint-test", 256, cpu_count, 1.0e-6, 1.0e-6, 10.0)
        .expect("test optimizer settings should be valid")
}

fn build_test_active_parameter_layout() -> JointRefinementActiveParameterLayout {
    JointRefinementActiveParameterLayout {
        hawkes_block: vec![0, 1, 2, 3],
        nig_block: vec![4, 5, 6, 7],
        return_clock_block: vec![11, 12],
        long_variance_block: vec![13, 14, 15, 16, 17],
        option_pricing_kernel_block: vec![18, 19, 20],
        coupling_block: vec![8, 9, 10],
        log_stationary_event_rate_scale_index: 0,
        log_beta_scale_index: 1,
        log_self_excitation_scale_index: 2,
        log_cross_excitation_scale_index: 3,
        log_buy_excitation_return_clock_scale_index: 11,
        log_sell_excitation_return_clock_scale_index: 12,
        log_long_variance_mean_reversion_rate_scale_index: 13,
        long_variance_vol_of_vol_index: 14,
        long_variance_return_energy_loading_index: 15,
        long_variance_buy_event_loading_index: 16,
        long_variance_sell_event_loading_index: 17,
        option_pricing_log_buy_activity_premium_index: 18,
        option_pricing_log_sell_activity_premium_index: 19,
        option_pricing_pressure_signed_imbalance_exposure_risk_price_index: 20,
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

fn build_test_observed_data() -> JointRefinementObservedData {
    let observed_tick_return_scale_summary = JointRefinementReturnScaleSummary {
        mean_absolute_return: 0.01,
        realized_variance: 0.0006,
        absolute_return_quantile_q90: 0.012,
    };

    let observed_5m_return_scale_summary = JointRefinementReturnScaleSummary {
        mean_absolute_return: 0.02,
        realized_variance: 0.0012,
        absolute_return_quantile_q90: 0.024,
    };

    let observed_15m_return_scale_summary = JointRefinementReturnScaleSummary {
        mean_absolute_return: 0.03,
        realized_variance: 0.0018,
        absolute_return_quantile_q90: 0.036,
    };

    let observed_tick_continuous_return_calibration_sample = build_test_calibration_sample();
    let observed_tick_continuous_return_optimizer_context =
        build_continuous_return_optimizer_context(
            &observed_tick_continuous_return_calibration_sample,
        );
    let weighted_tick_continuous_return_optimizer_context =
        build_weighted_continuous_return_optimizer_context(
            &observed_tick_continuous_return_calibration_sample,
        );

    JointRefinementObservedData {
        observed_open_event_rate_per_hour: 1.0,
        fallback_sell_share_proxy: 0.5,
        observed_tick_continuous_return_calibration_sample,
        observed_tick_continuous_return_optimizer_context,
        weighted_tick_continuous_return_optimizer_context,
        observed_5m_intervals: vec![
            build_test_interval_observation(0, 300, 0.012, 2, 0.5),
            build_test_interval_observation(300, 600, -0.009, 2, 0.5),
        ],
        observed_15m_intervals: vec![
            build_test_interval_observation(0, 900, 0.018, 6, 0.5),
            build_test_interval_observation(900, 1800, -0.012, 5, 0.5),
        ],
        observed_1h_intervals: vec![
            build_test_interval_observation(0, 3600, 0.018, 6, 0.5),
            build_test_interval_observation(3600, 7200, -0.012, 5, 0.5),
        ],
        observed_1d_intervals: vec![build_test_interval_observation(0, 86_400, 0.006, 11, 0.5)],
        observed_tick_intraday_mean_absolute_returns: vec![0.01; 24],
        observed_tick_shape_returns: vec![0.012, -0.009, 0.006, -0.004, 0.011, -0.007],
        observed_5m_shape_returns: vec![0.012, -0.009],
        observed_15m_shape_returns: vec![0.018, -0.012],
        observed_1h_shape_returns: vec![0.018, -0.012],
        observed_1d_shape_returns: vec![0.006],
        observed_1w_shape_returns: vec![0.006],
        observed_tick_return_scale_summary,
        observed_1h_return_scale_summary: observed_15m_return_scale_summary,
        observed_1d_return_scale_summary: JointRefinementReturnScaleSummary {
            mean_absolute_return: 0.006,
            realized_variance: 0.000036,
            absolute_return_quantile_q90: 0.006,
        },
        observed_5m_business_time_bucket_thresholds: JointRefinementBusinessTimeBucketThresholds {
            lower_quartile_cutoff: 0.8,
            median_cutoff: 1.0,
            upper_quartile_cutoff: 1.2,
        },
        observed_15m_business_time_bucket_thresholds: JointRefinementBusinessTimeBucketThresholds {
            lower_quartile_cutoff: 0.8,
            median_cutoff: 1.0,
            upper_quartile_cutoff: 1.2,
        },
        observed_5m_business_time_return_scale_summaries:
            JointRefinementBusinessTimeBucketReturnScaleSummaries {
                bucket_summaries: [Some(observed_5m_return_scale_summary); 4],
            },
        observed_15m_business_time_return_scale_summaries:
            JointRefinementBusinessTimeBucketReturnScaleSummaries {
                bucket_summaries: [Some(observed_15m_return_scale_summary); 4],
            },
        observed_5m_intraday_mean_absolute_returns: vec![0.02; 24],
        observed_15m_intraday_mean_absolute_returns: vec![0.03; 24],
        observed_hourly_tick_counts: vec![1.0; 24],
        observed_15m_rv_activity_metric: JointRefinementRollingVarianceActivityMetric {
            correlation: 0.0,
            standardized_path_distance: 0.0,
        },
        observed_long_variance_horizon_summaries: long_variance_horizon_summaries(
            LongVarianceHorizonInputIntervals {
                five_minute_return_intervals: &[
                    build_test_interval_observation(0, 300, 0.012, 2, 0.5),
                    build_test_interval_observation(300, 600, -0.009, 2, 0.5),
                ],
                five_minute_activity_intervals: &[
                    build_test_interval_observation(0, 300, 0.012, 2, 0.5),
                    build_test_interval_observation(300, 600, -0.009, 2, 0.5),
                ],
                hourly_return_intervals: &[
                    build_test_interval_observation(0, 900, 0.018, 6, 0.5),
                    build_test_interval_observation(900, 1800, -0.012, 5, 0.5),
                ],
                hourly_activity_intervals: &[
                    build_test_interval_observation(0, 900, 0.018, 6, 0.5),
                    build_test_interval_observation(900, 1800, -0.012, 5, 0.5),
                ],
            },
        ),
        hourly_simulation_start_utc: DateTime::<Utc>::from_timestamp_nanos(0),
        hourly_simulation_end_utc: DateTime::<Utc>::from_timestamp_nanos(24 * 3_600_000_000_000),
        hourly_simulation_interval_count: 24,
        surprise_deterministic_intensity_profile: SurpriseNewsDeterministicIntensityProfile {
            weekly_utc_profile_multipliers: vec![1.0; 24 * 7],
            profile_resolution_hours: 1,
            fit_policy: "test_weekly_utc_profile.v1".to_string(),
            normalization_policy: "test_unit_mean.v1".to_string(),
        },
        simulated_5m_interval_template: vec![
            build_test_interval_observation(0, 300, 0.0, 0, 0.5),
            build_test_interval_observation(300, 600, 0.0, 0, 0.5),
        ],
        simulated_15m_interval_template: vec![
            build_test_interval_observation(0, 900, 0.0, 0, 0.5),
            build_test_interval_observation(900, 1800, 0.0, 0, 0.5),
        ],
        simulated_hourly_interval_template: vec![
            build_test_interval_observation(
                0,
                3_600_000_000_000,
                0.0,
                0,
                0.5
            );
            24
        ],
    }
}

fn build_test_objective_term(
    metric_id: &str,
    observed_value: f64,
    simulated_value: f64,
) -> JointRefinementStageObjectiveTermDiagnostic {
    JointRefinementStageObjectiveTermDiagnostic {
        term_id: metric_id.to_string(),
        family_id: "return_scale".to_string(),
        term_kind: CalibrationLossTermKind::DiagnosticCoordinate,
        observed_value: Some(observed_value),
        model_value: Some(simulated_value),
        observed_standard_error: Some(observed_value.abs()),
        model_standard_error: Some(0.0),
        uncertainty_scale: Some(observed_value.abs()),
        standardized_residual: Some(0.0),
        covariance_diagonal: Some(observed_value * observed_value),
        loss: 0.0,
    }
}

fn build_test_joint_breakdown(
    tick_return_asymmetry_fit: f64,
    tick_return_scale_fit: f64,
    global_loss: f64,
) -> JointRefinementStageObjectiveBreakdown {
    let family_losses = [
        ("aggregated_likelihood_fit", 1.0),
        ("aggregated_return_shape_fit", 1.0),
        ("return_asymmetry_fit", 1.0),
        ("tick_return_asymmetry_fit", tick_return_asymmetry_fit),
        ("aggregated_return_scale_fit", 1.0),
        ("tick_return_scale_fit", tick_return_scale_fit),
        ("business_time_return_scale_fit", 1.0),
        ("intraday_return_level_fit", 1.0),
        ("activity_shape_fit", 1.0),
        ("activity_level_fit", 1.0),
        ("rv_activity_coupling_fit", 1.0),
        ("long_variance_horizon_fit", 1.0),
    ]
    .into_iter()
    .map(|(family_id, loss)| CalibrationLossFamilyDiagnostic {
        family_id: family_id.to_string(),
        coordinate_count: 1,
        rank: 1,
        whitened_coordinates: vec![loss.sqrt()],
        loss,
        max_standardized_residual: loss,
        worst_term_id: Some(family_id.to_string()),
        unidentified: false,
    })
    .collect::<Vec<_>>();

    JointRefinementStageObjectiveBreakdown {
        global_loss,
        family_losses,
        term_diagnostics: Vec::new(),
        domain_margin_diagnostics: Vec::new(),
        admissibility: CalibrationLossAdmissibility {
            admissible: true,
            max_standardized_residual: 1.0,
            excess_loss: 0.0,
            warnings: Vec::new(),
        },
    }
}

fn build_test_invalid_joint_breakdown(
    tick_return_asymmetry_fit: f64,
    tick_return_scale_fit: f64,
    invalid_term_id: &str,
) -> JointRefinementStageObjectiveBreakdown {
    let mut breakdown = build_test_joint_breakdown(
        tick_return_asymmetry_fit,
        tick_return_scale_fit,
        f64::INFINITY,
    );
    breakdown
        .term_diagnostics
        .push(CalibrationLossTermDiagnostic {
            term_id: invalid_term_id.to_string(),
            family_id: "invalid_candidate".to_string(),
            term_kind: CalibrationLossTermKind::InvalidCandidate,
            observed_value: None,
            model_value: None,
            observed_standard_error: None,
            model_standard_error: None,
            uncertainty_scale: None,
            standardized_residual: None,
            covariance_diagonal: None,
            loss: f64::INFINITY,
        });
    breakdown.admissibility = CalibrationLossAdmissibility {
        admissible: false,
        max_standardized_residual: 4.0,
        excess_loss: f64::INFINITY,
        warnings: Vec::new(),
    };
    breakdown
}

fn build_test_raw_admissibility_metrics(
    breakdown: &JointRefinementStageObjectiveBreakdown,
) -> JointRefinementRawAdmissibilityMetrics {
    let family_loss = |family_id: &str| {
        breakdown
            .family_losses
            .iter()
            .find(|family| family.family_id == family_id)
            .map(|family| family.loss)
            .unwrap_or(1.0)
    };

    JointRefinementRawAdmissibilityMetrics {
        aggregated_likelihood_fit: family_loss("aggregated_likelihood_fit"),
        aggregated_return_shape_fit: family_loss("aggregated_return_shape_fit"),
        return_asymmetry_fit: family_loss("return_asymmetry_fit"),
        tick_return_asymmetry_fit: family_loss("tick_return_asymmetry_fit"),
        aggregated_return_scale_fit: family_loss("aggregated_return_scale_fit"),
        tick_return_scale_fit: family_loss("tick_return_scale_fit"),
        business_time_return_scale_fit: family_loss("business_time_return_scale_fit"),
        intraday_return_level_fit: family_loss("intraday_return_level_fit"),
        activity_shape_fit: family_loss("activity_shape_fit"),
        activity_level_fit: family_loss("activity_level_fit"),
        rv_activity_coupling_fit: family_loss("rv_activity_coupling_fit"),
        long_variance_horizon_fit: family_loss("long_variance_horizon_fit"),
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

fn build_test_restart_outcome(
    restart_id: &str,
    breakdown: JointRefinementStageObjectiveBreakdown,
) -> JointRefinementRestartOutcome {
    let evaluation = JointRefinementCandidateEvaluation {
        fitted_parameters: test_joint_fitted_parameters(),
        raw_admissibility_metrics: build_test_raw_admissibility_metrics(&breakdown),
        breakdown,
        exact_likelihood_evaluator_counts: ContinuousLikelihoodEvaluatorCounts::default(),
        objective_terms: Vec::new(),
        invalid_reason: None,
    };

    JointRefinementRestartOutcome {
        restart_id: restart_id.to_string(),
        parameter_vector: vec![0.0; 11],
        local_evaluation: evaluation.clone(),
        final_evaluation: evaluation,
        local_evaluation_count: 4,
        stop_reason: "test".to_string(),
    }
}

fn build_test_seed_summary(
    simulated_5m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_15m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_5m_return_scale_summary: JointRefinementReturnScaleSummary,
    simulated_15m_return_scale_summary: JointRefinementReturnScaleSummary,
) -> JointRefinementSeedSummary {
    let simulated_long_variance_horizon_summaries =
        long_variance_horizon_summaries(LongVarianceHorizonInputIntervals {
            five_minute_return_intervals: simulated_5m_intervals.as_slice(),
            five_minute_activity_intervals: simulated_5m_intervals.as_slice(),
            hourly_return_intervals: simulated_15m_intervals.as_slice(),
            hourly_activity_intervals: simulated_15m_intervals.as_slice(),
        });

    JointRefinementSeedSummary {
        simulated_5m_intervals,
        simulated_15m_intervals,
        simulated_tick_shape_returns: vec![0.01, -0.01],
        simulated_5m_shape_returns: vec![0.02, -0.02],
        simulated_15m_shape_returns: vec![0.03, -0.03],
        simulated_1h_shape_returns: vec![0.03, -0.03],
        simulated_1d_shape_returns: vec![0.0],
        simulated_1w_shape_returns: vec![0.0],
        simulated_tick_return_scale_summary: JointRefinementReturnScaleSummary {
            mean_absolute_return: 0.01,
            realized_variance: 0.0001,
            absolute_return_quantile_q90: 0.02,
        },
        simulated_5m_return_scale_summary,
        simulated_15m_return_scale_summary,
        simulated_1h_return_scale_summary: simulated_15m_return_scale_summary,
        simulated_1d_return_scale_summary: JointRefinementReturnScaleSummary {
            mean_absolute_return: 0.0,
            realized_variance: 0.0,
            absolute_return_quantile_q90: 0.0,
        },
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
        simulated_long_variance_horizon_summaries,
    }
}

#[test]
fn joint_refinement_desired_restart_candidate_budget_defaults_to_fifteen() {
    assert_eq!(
        desired_joint_refinement_restart_candidate_budget(None, None),
        15
    );
}

#[test]
fn joint_refinement_desired_restart_candidate_budget_honors_override() {
    assert_eq!(
        desired_joint_refinement_restart_candidate_budget(Some(9), None),
        9
    );
}

#[test]
fn joint_refinement_restart_candidate_budget_controls_frontier_width_and_search_depth() {
    let solver_settings = build_test_solver_settings(64);

    let single_restart_schedule = joint_refinement_budget_schedule(&solver_settings, 1, None);

    assert_eq!(single_restart_schedule.restart_count, 1);
    assert_eq!(
        single_restart_schedule.search_depth_multiplier,
        super::JOINT_REFINEMENT_PARAMETER_FAMILY_COUNT
    );

    let default_frontier_schedule = joint_refinement_budget_schedule(&solver_settings, 15, None);

    assert_eq!(default_frontier_schedule.restart_count, 15);
    assert_eq!(default_frontier_schedule.search_depth_multiplier, 2);

    let broad_frontier_schedule = joint_refinement_budget_schedule(&solver_settings, 50, None);

    assert_eq!(broad_frontier_schedule.restart_count, 50);
    assert_eq!(broad_frontier_schedule.search_depth_multiplier, 1);
}

#[test]
fn joint_refinement_baseline_parameter_vector_round_trips_selected_return_nig() {
    let active_parameter_layout = build_test_active_parameter_layout();

    let baseline_15m_calibration_sample = build_test_calibration_sample();

    let selected_search_point = NigShapeSearchPoint {
        log_alpha: 0.4,
        buy_skew_coordinate: -0.2,
        sell_relative_coordinate: 0.3,
        log_delta_offset: -0.1,
    };

    let selected_nig = nig_configuration_from_search_point(
        &baseline_15m_calibration_sample,
        selected_search_point,
    )
    .expect("selected NIG should decode from joint search point");

    let mut fitted_parameters = test_joint_fitted_parameters().return_components_fitted_parameters;

    fitted_parameters.nig_shared_plus_shift_configuration = selected_nig;

    let baseline_parameter_vector = build_baseline_parameter_vector(
        &active_parameter_layout,
        &test_joint_fitted_parameters().hawkes_fitted_parameters,
        &fitted_parameters,
        &baseline_15m_calibration_sample,
    )
    .expect("representable selected NIG should build a baseline vector");

    let decoded_search_point =
        parameter_vector_to_nig_search_point(&baseline_parameter_vector, &active_parameter_layout);
    let expected_search_point =
        nig_search_point_from_configuration(&baseline_15m_calibration_sample, selected_nig)
            .expect("selected NIG should map back into joint search coordinates");

    let decoded_nig =
        nig_configuration_from_search_point(&baseline_15m_calibration_sample, decoded_search_point)
            .expect("baseline vector NIG block should decode");

    assert_eq!(decoded_search_point, expected_search_point);
    assert_eq!(decoded_nig.mu, selected_nig.mu);
}

#[test]
fn joint_refinement_nig_bounds_include_selected_return_nig_outside_theoretical_range() {
    let active_parameter_layout = build_test_active_parameter_layout();

    let baseline_15m_calibration_sample = build_test_calibration_sample();

    let solver_settings = build_test_solver_settings(1);
    let (theoretical_lower_bounds, theoretical_upper_bounds) =
        nig_search_bounds_for_calibration_sample_with_solver_settings(
            &baseline_15m_calibration_sample,
            &solver_settings,
        );

    let selected_search_point = NigShapeSearchPoint {
        log_alpha: theoretical_lower_bounds[0] - 0.25,
        buy_skew_coordinate: 0.5 * (theoretical_lower_bounds[1] + theoretical_upper_bounds[1]),
        sell_relative_coordinate: 0.5 * (theoretical_lower_bounds[2] + theoretical_upper_bounds[2]),
        log_delta_offset: theoretical_upper_bounds[3] + 0.25,
    };

    let selected_nig = nig_configuration_from_search_point(
        &baseline_15m_calibration_sample,
        selected_search_point,
    )
    .expect("selected NIG should decode from widened-search test point");

    let mut fitted_parameters = test_joint_fitted_parameters().return_components_fitted_parameters;

    fitted_parameters.nig_shared_plus_shift_configuration = selected_nig;

    let baseline_parameter_vector = build_baseline_parameter_vector(
        &active_parameter_layout,
        &test_joint_fitted_parameters().hawkes_fitted_parameters,
        &fitted_parameters,
        &baseline_15m_calibration_sample,
    )
    .expect("representable selected NIG should build a baseline vector");

    let (lower_bounds, upper_bounds) = build_parameter_bounds(ParameterBoundsBuildInput {
        active_parameter_layout: &active_parameter_layout,
        baseline_parameter_vector: baseline_parameter_vector.as_slice(),
        baseline_15m_calibration_sample: &baseline_15m_calibration_sample,
        selected_return_nig_configuration: selected_nig,
        return_component_basin_diagnostics: &[],
        baseline_hawkes_kernel_parameters: test_joint_fitted_parameters()
            .hawkes_fitted_parameters
            .kernel_parameters,
        baseline_long_variance_parameters: test_joint_fitted_parameters()
            .return_components_fitted_parameters
            .long_variance_parameters,
        observed_data: &build_test_observed_data(),
        baseline_modeled_open_event_rate_per_hour: 1.0,
        solver_settings: &solver_settings,
    })
    .expect("bounds should include selected return-stage NIG");

    for parameter_index in &active_parameter_layout.nig_block {
        assert!(
            baseline_parameter_vector[*parameter_index] >= lower_bounds[*parameter_index]
                && baseline_parameter_vector[*parameter_index] <= upper_bounds[*parameter_index]
        );
    }

    assert!(lower_bounds[active_parameter_layout.nig_block[0]] < theoretical_lower_bounds[0]);
    assert!(upper_bounds[active_parameter_layout.nig_block[3]] > theoretical_upper_bounds[3]);
}

#[test]
fn joint_refinement_bounds_allow_option_pricing_kernel_activity_premia() {
    let active_parameter_layout = build_test_active_parameter_layout();

    let baseline_15m_calibration_sample = build_test_calibration_sample();

    let selected_nig = test_joint_fitted_parameters()
        .return_components_fitted_parameters
        .nig_shared_plus_shift_configuration;

    let baseline_parameter_vector = build_baseline_parameter_vector(
        &active_parameter_layout,
        &test_joint_fitted_parameters().hawkes_fitted_parameters,
        &test_joint_fitted_parameters().return_components_fitted_parameters,
        &baseline_15m_calibration_sample,
    )
    .expect("baseline vector should build");

    let (lower_bounds, upper_bounds) = build_parameter_bounds(ParameterBoundsBuildInput {
        active_parameter_layout: &active_parameter_layout,
        baseline_parameter_vector: baseline_parameter_vector.as_slice(),
        baseline_15m_calibration_sample: &baseline_15m_calibration_sample,
        selected_return_nig_configuration: selected_nig,
        return_component_basin_diagnostics: &[],
        baseline_hawkes_kernel_parameters: test_joint_fitted_parameters()
            .hawkes_fitted_parameters
            .kernel_parameters,
        baseline_long_variance_parameters: test_joint_fitted_parameters()
            .return_components_fitted_parameters
            .long_variance_parameters,
        observed_data: &build_test_observed_data(),
        baseline_modeled_open_event_rate_per_hour: 1.0,
        solver_settings: &build_test_solver_settings(1),
    })
    .expect("bounds should build");

    for parameter_index in &active_parameter_layout.option_pricing_kernel_block {
        assert_eq!(
            lower_bounds[*parameter_index],
            -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND
        );
        assert_eq!(
            upper_bounds[*parameter_index],
            JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND
        );
    }
}

#[test]
fn joint_refinement_active_layout_includes_long_variance_coordinates() {
    let active_parameter_layout = build_active_parameter_layout(
        &test_joint_fitted_parameters().return_components_fitted_parameters,
    );

    assert_eq!(active_parameter_layout.long_variance_block.len(), 5);
    assert_eq!(
        active_parameter_layout.long_variance_block,
        vec![
            active_parameter_layout.log_long_variance_mean_reversion_rate_scale_index,
            active_parameter_layout.long_variance_vol_of_vol_index,
            active_parameter_layout.long_variance_return_energy_loading_index,
            active_parameter_layout.long_variance_buy_event_loading_index,
            active_parameter_layout.long_variance_sell_event_loading_index,
        ]
    );
}

#[test]
fn joint_refinement_baseline_parameter_vector_encodes_long_variance_parameters() {
    let active_parameter_layout = build_test_active_parameter_layout();

    let baseline_15m_calibration_sample = build_test_calibration_sample();

    let mut fitted_parameters = test_joint_fitted_parameters().return_components_fitted_parameters;

    fitted_parameters.long_variance_parameters =
        janiform_model_kernels::long_variance::LongVarianceParameters::try_new(
            2.0, 0.4, 0.03, 0.04, 0.01, 0.02,
        )
        .expect("test long variance parameters should construct");

    let baseline_parameter_vector = build_baseline_parameter_vector(
        &active_parameter_layout,
        &test_joint_fitted_parameters().hawkes_fitted_parameters,
        &fitted_parameters,
        &baseline_15m_calibration_sample,
    )
    .expect("baseline vector should build");

    assert_eq!(
        baseline_parameter_vector
            [active_parameter_layout.log_long_variance_mean_reversion_rate_scale_index],
        0.0
    );
    assert_eq!(
        baseline_parameter_vector[active_parameter_layout.long_variance_vol_of_vol_index],
        0.4
    );
    assert_eq!(
        baseline_parameter_vector
            [active_parameter_layout.long_variance_return_energy_loading_index],
        0.04
    );
    assert_eq!(
        baseline_parameter_vector[active_parameter_layout.long_variance_buy_event_loading_index],
        0.01
    );
    assert_eq!(
        baseline_parameter_vector[active_parameter_layout.long_variance_sell_event_loading_index],
        0.02
    );
}

#[test]
fn joint_refinement_bounds_allow_long_variance_search_coordinates() {
    let active_parameter_layout = build_test_active_parameter_layout();

    let baseline_15m_calibration_sample = build_test_calibration_sample();

    let selected_nig = test_joint_fitted_parameters()
        .return_components_fitted_parameters
        .nig_shared_plus_shift_configuration;

    let baseline_parameter_vector = build_baseline_parameter_vector(
        &active_parameter_layout,
        &test_joint_fitted_parameters().hawkes_fitted_parameters,
        &test_joint_fitted_parameters().return_components_fitted_parameters,
        &baseline_15m_calibration_sample,
    )
    .expect("baseline vector should build");

    let (lower_bounds, upper_bounds) = build_parameter_bounds(ParameterBoundsBuildInput {
        active_parameter_layout: &active_parameter_layout,
        baseline_parameter_vector: baseline_parameter_vector.as_slice(),
        baseline_15m_calibration_sample: &baseline_15m_calibration_sample,
        selected_return_nig_configuration: selected_nig,
        return_component_basin_diagnostics: &[],
        baseline_hawkes_kernel_parameters: test_joint_fitted_parameters()
            .hawkes_fitted_parameters
            .kernel_parameters,
        baseline_long_variance_parameters: test_joint_fitted_parameters()
            .return_components_fitted_parameters
            .long_variance_parameters,
        observed_data: &build_test_observed_data(),
        baseline_modeled_open_event_rate_per_hour: 1.0,
        solver_settings: &build_test_solver_settings(1),
    })
    .expect("bounds should build");

    for parameter_index in &active_parameter_layout.long_variance_block {
        assert!(
            baseline_parameter_vector[*parameter_index] >= lower_bounds[*parameter_index]
                && baseline_parameter_vector[*parameter_index] <= upper_bounds[*parameter_index]
        );
    }

    assert_eq!(
        lower_bounds[active_parameter_layout.log_long_variance_mean_reversion_rate_scale_index],
        -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND
    );
    assert_eq!(
        upper_bounds[active_parameter_layout.log_long_variance_mean_reversion_rate_scale_index],
        JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND
    );
    assert_eq!(
        lower_bounds[active_parameter_layout.long_variance_vol_of_vol_index],
        0.0
    );
    assert!(upper_bounds[active_parameter_layout.long_variance_vol_of_vol_index] > 0.0);
}

fn restart_seed_ids(restart_seeds: &[(String, Vec<f64>)]) -> Vec<&str> {
    restart_seeds
        .iter()
        .map(|(restart_id, _)| restart_id.as_str())
        .collect()
}

fn restart_seed_by_id<'a>(
    restart_seeds: &'a [(String, Vec<f64>)],
    expected_restart_id: &str,
) -> &'a [f64] {
    restart_seeds
        .iter()
        .find(|(restart_id, _)| restart_id == expected_restart_id)
        .map(|(_, restart_seed)| restart_seed.as_slice())
        .expect("expected restart seed should be present")
}

#[test]
fn joint_refinement_restart_seed_generation_is_deterministic() {
    let active_parameter_layout = build_test_active_parameter_layout();

    let baseline_parameter_vector = vec![0.0; active_parameter_layout.vector_length()];

    let baseline_15m_calibration_sample = build_test_calibration_sample();

    let lower_bounds = vec![-1.0; baseline_parameter_vector.len()];
    let upper_bounds = vec![2.0; baseline_parameter_vector.len()];

    let first_restart_seeds =
        build_restart_seeds_with_halton_acceptance(RestartSeedsWithHaltonAcceptanceBuildInput {
            baseline_parameter_vector: baseline_parameter_vector.as_slice(),
            active_parameter_layout: &active_parameter_layout,
            lower_bounds: lower_bounds.as_slice(),
            upper_bounds: upper_bounds.as_slice(),
            baseline_15m_calibration_sample: &baseline_15m_calibration_sample,
            return_component_basin_diagnostics: &[],
            baseline_final_objective_terms: &[],
            activity_log_shift: 0.25,
            restart_candidate_budget: 12,
            halton_seed_is_accepted: Box::new(|_| true),
        });

    let second_restart_seeds =
        build_restart_seeds_with_halton_acceptance(RestartSeedsWithHaltonAcceptanceBuildInput {
            baseline_parameter_vector: baseline_parameter_vector.as_slice(),
            active_parameter_layout: &active_parameter_layout,
            lower_bounds: lower_bounds.as_slice(),
            upper_bounds: upper_bounds.as_slice(),
            baseline_15m_calibration_sample: &baseline_15m_calibration_sample,
            return_component_basin_diagnostics: &[],
            baseline_final_objective_terms: &[],
            activity_log_shift: 0.25,
            restart_candidate_budget: 12,
            halton_seed_is_accepted: Box::new(|_| true),
        });

    assert_eq!(first_restart_seeds, second_restart_seeds);
    assert_eq!(first_restart_seeds.len(), 12);
    assert_eq!(first_restart_seeds[0].0, "incumbent_local_search");

    let restart_seed_ids = restart_seed_ids(first_restart_seeds.as_slice());

    assert!(restart_seed_ids.contains(&"activity_aligned"));
    assert!(restart_seed_ids.contains(&"midpoint"));
    assert!(
        restart_seed_ids
            .iter()
            .any(|restart_id| restart_id.starts_with("halton_"))
    );
}

#[test]
fn joint_refinement_restart_seed_generation_adds_return_scale_alignment_seeds() {
    let active_parameter_layout = build_test_active_parameter_layout();

    let baseline_parameter_vector = vec![0.0; active_parameter_layout.vector_length()];

    let baseline_15m_calibration_sample = build_test_calibration_sample();

    let lower_bounds = vec![-1.0; baseline_parameter_vector.len()];
    let upper_bounds = vec![2.0; baseline_parameter_vector.len()];

    let baseline_final_objective_terms = vec![
        build_test_objective_term("return_scale.1d.mean_absolute_return", 1.0, 2.0),
        build_test_objective_term("return_scale.tick.realized_variance", 4.0, 25.0),
    ];

    let restart_seeds =
        build_restart_seeds_with_halton_acceptance(RestartSeedsWithHaltonAcceptanceBuildInput {
            baseline_parameter_vector: baseline_parameter_vector.as_slice(),
            active_parameter_layout: &active_parameter_layout,
            lower_bounds: lower_bounds.as_slice(),
            upper_bounds: upper_bounds.as_slice(),
            baseline_15m_calibration_sample: &baseline_15m_calibration_sample,
            return_component_basin_diagnostics: &[],
            baseline_final_objective_terms: baseline_final_objective_terms.as_slice(),
            activity_log_shift: 0.25,
            restart_candidate_budget: 18,
            halton_seed_is_accepted: Box::new(|_| false),
        });

    let log_stationary_event_rate_index =
        active_parameter_layout.log_stationary_event_rate_scale_index;

    let log_delta_offset_index = active_parameter_layout.nig_block[3];

    let mean_absolute_return_delta_shift = 0.5f64.ln();
    let tick_realized_variance_delta_shift = 0.4f64.ln();

    assert_eq!(restart_seeds.len(), 14);

    let mean_absolute_return_seed = restart_seed_by_id(
        restart_seeds.as_slice(),
        "return_scale_mean_absolute_return_1d",
    );

    assert_eq!(
        mean_absolute_return_seed[log_delta_offset_index],
        mean_absolute_return_delta_shift
    );

    let mean_absolute_return_activity_seed = restart_seed_by_id(
        restart_seeds.as_slice(),
        "return_scale_mean_absolute_return_1d_activity_aligned",
    );

    assert_eq!(
        mean_absolute_return_activity_seed[log_delta_offset_index],
        mean_absolute_return_delta_shift
    );
    assert_eq!(
        mean_absolute_return_activity_seed[log_stationary_event_rate_index],
        0.0
    );

    let tick_realized_variance_seed = restart_seed_by_id(
        restart_seeds.as_slice(),
        "return_scale_tick_realized_variance",
    );

    assert_eq!(
        tick_realized_variance_seed[log_delta_offset_index],
        tick_realized_variance_delta_shift
    );
}

#[test]
fn joint_refinement_restart_seed_generation_counts_only_accepted_haltons() {
    let active_parameter_layout = build_test_active_parameter_layout();

    let baseline_parameter_vector = vec![0.0; active_parameter_layout.vector_length()];

    let baseline_15m_calibration_sample = build_test_calibration_sample();

    let lower_bounds = vec![-1.0; baseline_parameter_vector.len()];
    let upper_bounds = vec![2.0; baseline_parameter_vector.len()];

    let mut score_call_count = 0usize;

    let restart_seeds =
        build_restart_seeds_with_halton_acceptance(RestartSeedsWithHaltonAcceptanceBuildInput {
            baseline_parameter_vector: baseline_parameter_vector.as_slice(),
            active_parameter_layout: &active_parameter_layout,
            lower_bounds: lower_bounds.as_slice(),
            upper_bounds: upper_bounds.as_slice(),
            baseline_15m_calibration_sample: &baseline_15m_calibration_sample,
            return_component_basin_diagnostics: &[],
            baseline_final_objective_terms: &[],
            activity_log_shift: 0.25,
            restart_candidate_budget: 12,
            halton_seed_is_accepted: Box::new(|_| {
                score_call_count += 1;
                score_call_count.is_multiple_of(3)
            }),
        });

    assert_eq!(restart_seeds.len(), 12);

    let restart_seed_ids = restart_seed_ids(restart_seeds.as_slice());
    let halton_restart_seed_ids = restart_seed_ids
        .iter()
        .copied()
        .filter(|restart_id| restart_id.starts_with("halton_"))
        .collect::<Vec<_>>();

    assert!(!halton_restart_seed_ids.is_empty());
    assert!(halton_restart_seed_ids.iter().all(|restart_id| {
        restart_id
            .strip_prefix("halton_")
            .and_then(|suffix| suffix.parse::<usize>().ok())
            .is_some_and(|halton_position| halton_position.is_multiple_of(3))
    }));
    assert!(score_call_count >= 9);
}

#[test]
fn joint_refinement_restart_seed_generation_uses_restart_candidate_exploration_budget() {
    let active_parameter_layout = build_test_active_parameter_layout();

    let baseline_parameter_vector = vec![0.0; active_parameter_layout.vector_length()];

    let baseline_15m_calibration_sample = build_test_calibration_sample();

    let lower_bounds = vec![-1.0; baseline_parameter_vector.len()];
    let upper_bounds = vec![2.0; baseline_parameter_vector.len()];

    let mut score_call_count = 0usize;

    let restart_seeds =
        build_restart_seeds_with_halton_acceptance(RestartSeedsWithHaltonAcceptanceBuildInput {
            baseline_parameter_vector: baseline_parameter_vector.as_slice(),
            active_parameter_layout: &active_parameter_layout,
            lower_bounds: lower_bounds.as_slice(),
            upper_bounds: upper_bounds.as_slice(),
            baseline_15m_calibration_sample: &baseline_15m_calibration_sample,
            return_component_basin_diagnostics: &[],
            baseline_final_objective_terms: &[],
            activity_log_shift: 0.25,
            restart_candidate_budget: 12,
            halton_seed_is_accepted: Box::new(|_| {
                score_call_count += 1;
                false
            }),
        });

    assert_eq!(restart_seeds.len(), 10);
    assert_eq!(score_call_count, 12);

    let restart_seed_ids = restart_seed_ids(restart_seeds.as_slice());

    assert!(restart_seed_ids.contains(&"long_variance_vol_of_vol_up"));
}

#[test]
fn joint_refinement_restart_candidate_screening_uses_configured_parallelism() {
    let max_parallelism = restart_candidate_screening_max_parallelism_for_test(4, 8);

    assert!(max_parallelism > 1);
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

    let restart_seeds =
        build_restart_seeds_with_halton_acceptance(RestartSeedsWithHaltonAcceptanceBuildInput {
            baseline_parameter_vector: baseline_parameter_vector.as_slice(),
            active_parameter_layout: &active_parameter_layout,
            lower_bounds: lower_bounds.as_slice(),
            upper_bounds: upper_bounds.as_slice(),
            baseline_15m_calibration_sample: &baseline_15m_calibration_sample,
            return_component_basin_diagnostics: basin_diagnostics.as_slice(),
            baseline_final_objective_terms: &[],
            activity_log_shift: 0.25,
            restart_candidate_budget: 18,
            halton_seed_is_accepted: Box::new(|_| true),
        });

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

    let nig_basin_seed = restart_seed_by_id(restart_seeds.as_slice(), "nig_basin_skew_heavy");

    let nig_basin_activity_seed = restart_seed_by_id(
        restart_seeds.as_slice(),
        "nig_basin_skew_heavy_activity_aligned",
    );

    assert_eq!(
        nig_basin_seed[active_parameter_layout.nig_block[0]],
        clamped_projected_coordinates[0]
    );
    assert_eq!(
        nig_basin_seed[active_parameter_layout.nig_block[1]],
        clamped_projected_coordinates[1]
    );
    assert_eq!(
        nig_basin_seed[active_parameter_layout.nig_block[2]],
        clamped_projected_coordinates[2]
    );
    assert_eq!(
        nig_basin_seed[active_parameter_layout.nig_block[3]],
        clamped_projected_coordinates[3]
    );
    assert_eq!(
        nig_basin_activity_seed[active_parameter_layout.log_stationary_event_rate_scale_index],
        0.0
    );
    assert_eq!(
        nig_basin_activity_seed[active_parameter_layout
            .known_activity_scale_index
            .expect("known activity scale should be present")],
        0.25
    );
    assert_eq!(
        nig_basin_activity_seed[active_parameter_layout
            .surprise_activity_scale_index
            .expect("surprise activity scale should be present")],
        0.25
    );
    assert_eq!(
        nig_basin_activity_seed[active_parameter_layout
            .surprise_baseline_scale_index
            .expect("surprise baseline scale should be present")],
        0.25
    );

    let nig_alpha_delta_seed =
        restart_seed_by_id(restart_seeds.as_slice(), "nig_alpha_delta_skew_heavy");

    let nig_alpha_delta_activity_seed = restart_seed_by_id(
        restart_seeds.as_slice(),
        "nig_alpha_delta_skew_heavy_activity_aligned",
    );

    assert_eq!(
        nig_alpha_delta_seed[active_parameter_layout.nig_block[0]],
        clamped_projected_coordinates[0]
    );
    assert_eq!(
        nig_alpha_delta_seed[active_parameter_layout.nig_block[1]],
        baseline_parameter_vector[active_parameter_layout.nig_block[1]]
    );
    assert_eq!(
        nig_alpha_delta_seed[active_parameter_layout.nig_block[2]],
        baseline_parameter_vector[active_parameter_layout.nig_block[2]]
    );
    assert_eq!(
        nig_alpha_delta_seed[active_parameter_layout.nig_block[3]],
        clamped_projected_coordinates[3]
    );
    assert_eq!(
        nig_alpha_delta_activity_seed
            [active_parameter_layout.log_stationary_event_rate_scale_index],
        0.0
    );
    assert_eq!(
        nig_alpha_delta_activity_seed[active_parameter_layout
            .known_activity_scale_index
            .expect("known activity scale should be present")],
        0.25
    );
    assert_eq!(
        nig_alpha_delta_activity_seed[active_parameter_layout
            .surprise_activity_scale_index
            .expect("surprise activity scale should be present")],
        0.25
    );
    assert_eq!(
        nig_alpha_delta_activity_seed[active_parameter_layout
            .surprise_baseline_scale_index
            .expect("surprise baseline scale should be present")],
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
            elapsed_same_session_hours: 1.0,
            log_return: 0.01,
            post_shock_activity_lift: 0.0,
            sell_share_proxy: 0.5,
            source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.5),
        },
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 2,
            elapsed_same_session_hours: 1.0,
            log_return: 0.02,
            post_shock_activity_lift: 0.0,
            sell_share_proxy: 0.5,
            source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.5),
        },
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 3,
            elapsed_same_session_hours: 1.0,
            log_return: 0.03,
            post_shock_activity_lift: 0.0,
            sell_share_proxy: 0.5,
            source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.5),
        },
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 4,
            elapsed_same_session_hours: 1.0,
            log_return: 0.04,
            post_shock_activity_lift: 0.0,
            sell_share_proxy: 0.5,
            source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.5),
        },
    ];

    let sampled_returns =
        sampled_observed_tick_returns(observed_open_session_returns.as_slice(), 3);

    assert_eq!(sampled_returns, vec![0.01, 0.02, 0.04]);
}

#[test]
fn tick_shape_sample_target_count_uses_q01_rank_quantum_per_temporal_block() {
    assert_eq!(tick_shape_sample_target_count(1_000_000, 24), 2_400);
    assert_eq!(tick_shape_sample_target_count(664_465, 168), 16_800);
    assert_eq!(tick_shape_sample_target_count(500, 24), 500);
    assert_eq!(tick_shape_sample_target_count(1_000_000, 0), 100);
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

    let ordering = compare_joint_candidate_breakdowns(JointCandidateBreakdownComparisonInput {
        left_pareto_rank: 1,
        left_breakdown: &better_tick_fit,
        left_admissible: true,
        left_restart_id: "better_tick_fit",
        right_pareto_rank: 1,
        right_breakdown: &worse_tick_fit,
        right_admissible: true,
        right_restart_id: "worse_tick_fit",
        has_admissible_candidate: true,
    });

    assert_eq!(ordering, Ordering::Less);
}

#[test]
fn admissible_candidate_ordering_uses_global_loss_after_primary_metrics_tie() {
    let lower_objective = build_test_joint_breakdown(1.0, 1.0, 0.9);
    let higher_objective = build_test_joint_breakdown(1.0, 1.0, 1.1);

    let ordering = compare_joint_candidate_breakdowns(JointCandidateBreakdownComparisonInput {
        left_pareto_rank: 1,
        left_breakdown: &lower_objective,
        left_admissible: true,
        left_restart_id: "lower_objective",
        right_pareto_rank: 1,
        right_breakdown: &higher_objective,
        right_admissible: true,
        right_restart_id: "higher_objective",
        has_admissible_candidate: true,
    });

    assert_eq!(ordering, Ordering::Less);
}

#[test]
fn admissible_candidate_ordering_rejects_inadmissible_restart_when_available() {
    let admissible_baseline = build_test_joint_breakdown(1.0, 1.0, 1.2);
    let inadmissible_restart = build_test_joint_breakdown(0.8, 0.8, 0.7);

    let ordering = compare_joint_candidate_breakdowns(JointCandidateBreakdownComparisonInput {
        left_pareto_rank: 1,
        left_breakdown: &admissible_baseline,
        left_admissible: true,
        left_restart_id: "baseline",
        right_pareto_rank: 1,
        right_breakdown: &inadmissible_restart,
        right_admissible: false,
        right_restart_id: "restart_refined",
        has_admissible_candidate: true,
    });

    assert_eq!(ordering, Ordering::Less);
}

#[test]
fn option_refinement_frontier_excludes_fallback_restart_outcomes() {
    let admissible_restart = build_test_restart_outcome(
        "admissible_restart",
        build_test_joint_breakdown(1.0, 1.0, 1.2),
    );

    let fallback_restart = build_test_restart_outcome(
        "fallback_restart",
        build_test_invalid_joint_breakdown(0.8, 0.8, "activity_level_fit"),
    );

    let selected_restart_outcomes = vec![&admissible_restart, &fallback_restart];

    let option_frontier = option_refinement_candidate_frontier_from_selected_outcomes(
        selected_restart_outcomes.as_slice(),
    );

    let frontier_ids = option_frontier
        .iter()
        .map(|restart_outcome| restart_outcome.restart_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(frontier_ids, vec!["admissible_restart"]);
}

#[test]
fn option_refinement_frontier_is_empty_without_admissible_restart_outcomes() {
    let fallback_restart = build_test_restart_outcome(
        "fallback_restart",
        build_test_invalid_joint_breakdown(0.8, 0.8, "activity_level_fit"),
    );

    let selected_restart_outcomes = vec![&fallback_restart];

    let option_frontier = option_refinement_candidate_frontier_from_selected_outcomes(
        selected_restart_outcomes.as_slice(),
    );

    assert!(option_frontier.is_empty());
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

#[test]
fn selected_restart_validation_rejects_inadmissible_candidate() {
    let restart_outcome = build_test_restart_outcome(
        "joint_refinement_midpoint",
        build_test_invalid_joint_breakdown(1.0, 1.0, "activity_level_fit"),
    );

    let failure = ensure_valid_selected_restart_outcome(&restart_outcome, 0, 0, true)
        .expect_err("inadmissible selected restart should fail validation");

    match failure {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput { error_message } => {
            assert!(
                error_message.contains("did not identify a finite-loss replacement"),
                "unexpected failure message: {error_message}"
            );
            assert!(
                error_message.contains("rejected_restart=joint_refinement_midpoint"),
                "unexpected failure message: {error_message}"
            );
            assert!(
                error_message.contains("invalid_term_ids=activity_level_fit"),
                "unexpected failure message: {error_message}"
            );
        }
        other => panic!("unexpected failure variant: {other:?}"),
    }
}
