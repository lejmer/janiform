use janiform_engine_risk::{
    SufficientStatisticsIntervalStatistics, SufficientStatisticsRiskPath,
    SufficientStatisticsRiskPathRecord, SufficientStatisticsRiskReturnComponents,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_validation::{
    SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
    SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY, ScaleConsistencyMetricComputationStatus,
    ScaleConsistencyMetricDatasetRole, ScaleConsistencyMetricDeltaBatchRequest,
    ScaleConsistencyMetricFailureReasonCode, ScaleConsistencyMetricScenarioInput,
    ScaleConsistencyMetricSpecification, aggregate_fine_risk_path_to_coarse_intervals,
    calculate_scale_consistency_metric_deltas,
};

#[test]
fn scale_consistency_metric_delta_calculator_computes_returns_volatility_and_event_metrics() {
    let request = ScaleConsistencyMetricDeltaBatchRequest {
        metric_specifications: vec![
            metric_spec("returns_central_variance"),
            metric_spec("returns_distribution_wasserstein_distance"),
            metric_spec_with_lags("volatility_abs_return_acf_distribution_distance", &[1]),
            metric_spec("open_jump_abs_tail_expected_shortfall_ratio"),
            metric_spec("closed_gap_abs_tail_expected_shortfall_ratio"),
            metric_spec("closed_gap_vs_open_jump_wasserstein_distance"),
        ],
        scenarios: vec![build_scenario_input(
            "scenario-baseline",
            "regime-baseline",
            "hourly",
        )],
    };

    let report =
        calculate_scale_consistency_metric_deltas(&request).expect("metric delta batch should run");

    assert_eq!(
        report.schema_version,
        SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION
    );
    assert_eq!(
        report.ordering_policy,
        SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY
    );
    assert_eq!(report.scenario_reports.len(), 1);

    let scenario_report = &report.scenario_reports[0];
    assert_eq!(scenario_report.scenario_id, "scenario-baseline");
    assert_eq!(scenario_report.parameter_set_id, "regime-baseline");
    assert_eq!(scenario_report.horizon_label, "hourly");
    assert_eq!(scenario_report.horizon_interval_count, 3);
    assert_eq!(scenario_report.coarse_interval_delta, 1.0);
    assert_eq!(scenario_report.fine_interval_delta, 0.5);
    assert_eq!(
        scenario_report.fine_subinterval_count_per_coarse_interval,
        2
    );
    assert_eq!(scenario_report.metric_deltas.len(), 6);

    let returns_variance = &scenario_report.metric_deltas[0];
    assert_eq!(returns_variance.metric_id, "returns_central_variance");
    assert_eq!(
        returns_variance.status,
        ScaleConsistencyMetricComputationStatus::Computed
    );
    assert_eq!(returns_variance.horizon_label, "hourly");
    assert_eq!(returns_variance.horizon_interval_count, 3);
    assert_close(
        returns_variance.coarse_value.expect("coarse value"),
        7.0 / 3.0,
    );
    assert_close(returns_variance.fine_value.expect("fine value"), 4.0);
    assert_close(
        returns_variance.delta_value.expect("delta value"),
        -5.0 / 3.0,
    );
    assert_close(
        returns_variance.absolute_delta.expect("absolute delta"),
        5.0 / 3.0,
    );

    let returns_distance = &scenario_report.metric_deltas[1];
    assert_eq!(
        returns_distance.metric_id,
        "returns_distribution_wasserstein_distance"
    );
    assert_eq!(returns_distance.coarse_value, None);
    assert_eq!(returns_distance.fine_value, None);
    assert_close(
        returns_distance.delta_value.expect("pairwise distance"),
        2.0 / 3.0,
    );
    assert_close(
        returns_distance
            .absolute_delta
            .expect("pairwise abs distance"),
        2.0 / 3.0,
    );

    let volatility_distance = &scenario_report.metric_deltas[2];
    assert_eq!(
        volatility_distance.metric_id,
        "volatility_abs_return_acf_distribution_distance"
    );
    assert_eq!(
        volatility_distance.metric_variant.as_deref(),
        Some("lag_grid_1")
    );
    assert_close(
        volatility_distance
            .delta_value
            .expect("volatility distance"),
        1.0 / 42.0,
    );

    let open_jump_ratio = &scenario_report.metric_deltas[3];
    assert_eq!(
        open_jump_ratio.metric_id,
        "open_jump_abs_tail_expected_shortfall_ratio"
    );
    assert_close(
        open_jump_ratio.coarse_value.expect("coarse open ratio"),
        40.0 / 37.0,
    );
    assert_close(
        open_jump_ratio.fine_value.expect("fine open ratio"),
        15.0 / 14.0,
    );
    assert_close(
        open_jump_ratio.delta_value.expect("open ratio delta"),
        (40.0 / 37.0) - (15.0 / 14.0),
    );

    let closed_gap_ratio = &scenario_report.metric_deltas[4];
    assert_eq!(
        closed_gap_ratio.metric_id,
        "closed_gap_abs_tail_expected_shortfall_ratio"
    );
    assert_close(
        closed_gap_ratio.coarse_value.expect("coarse closed ratio"),
        25.0 / 23.0,
    );
    assert_close(
        closed_gap_ratio.fine_value.expect("fine closed ratio"),
        15.0 / 14.0,
    );

    let contrast_distance = &scenario_report.metric_deltas[5];
    assert_eq!(
        contrast_distance.metric_id,
        "closed_gap_vs_open_jump_wasserstein_distance"
    );
    assert_close(
        contrast_distance
            .coarse_value
            .expect("coarse contrast distance"),
        1.0 / 3.0,
    );
    assert_close(
        contrast_distance
            .fine_value
            .expect("fine contrast distance"),
        0.0,
    );
    assert_close(
        contrast_distance.delta_value.expect("contrast delta"),
        1.0 / 3.0,
    );
}

#[test]
fn scale_consistency_metric_delta_calculator_emits_missing_metric_failures_clearly() {
    let sparse_scenario = build_sparse_scenario_input("scenario-sparse", "regime-sparse", "hourly");
    let request = ScaleConsistencyMetricDeltaBatchRequest {
        metric_specifications: vec![
            metric_spec("returns_central_variance"),
            metric_spec_with_lags("volatility_abs_return_acf_distribution_distance", &[1]),
            metric_spec("open_jump_abs_tail_expected_shortfall_ratio"),
            metric_spec("closed_gap_abs_tail_expected_shortfall_ratio"),
        ],
        scenarios: vec![sparse_scenario],
    };

    let report =
        calculate_scale_consistency_metric_deltas(&request).expect("missing metrics should report");
    let scenario_report = &report.scenario_reports[0];

    let returns_variance = &scenario_report.metric_deltas[0];
    assert_eq!(
        returns_variance.status,
        ScaleConsistencyMetricComputationStatus::Missing
    );
    assert_eq!(returns_variance.delta_value, None);
    assert_eq!(returns_variance.computation_failures.len(), 2);
    assert_eq!(
        returns_variance.computation_failures[0].dataset_role,
        ScaleConsistencyMetricDatasetRole::Coarse
    );
    assert_eq!(
        returns_variance.computation_failures[0].reason_code,
        ScaleConsistencyMetricFailureReasonCode::InsufficientReturnObservations
    );
    assert_eq!(
        returns_variance.computation_failures[1].dataset_role,
        ScaleConsistencyMetricDatasetRole::Fine
    );

    let volatility_distance = &scenario_report.metric_deltas[1];
    assert_eq!(
        volatility_distance.status,
        ScaleConsistencyMetricComputationStatus::Missing
    );
    assert_eq!(volatility_distance.computation_failures.len(), 2);
    for failure in &volatility_distance.computation_failures {
        assert_eq!(
            failure.reason_code,
            ScaleConsistencyMetricFailureReasonCode::InsufficientLaggedObservations
        );
        assert_eq!(failure.lag_interval_count, Some(1));
    }

    let open_jump_ratio = &scenario_report.metric_deltas[2];
    assert_eq!(
        open_jump_ratio.status,
        ScaleConsistencyMetricComputationStatus::Missing
    );
    assert_eq!(open_jump_ratio.computation_failures.len(), 2);
    assert_eq!(
        open_jump_ratio.computation_failures[0].reason_code,
        ScaleConsistencyMetricFailureReasonCode::MissingOpenJumpObservations
    );
    assert_eq!(
        open_jump_ratio.computation_failures[1].reason_code,
        ScaleConsistencyMetricFailureReasonCode::MissingOpenJumpObservations
    );

    let closed_gap_ratio = &scenario_report.metric_deltas[3];
    assert_eq!(
        closed_gap_ratio.status,
        ScaleConsistencyMetricComputationStatus::Missing
    );
    assert_eq!(closed_gap_ratio.computation_failures.len(), 2);
    assert_eq!(
        closed_gap_ratio.computation_failures[0].reason_code,
        ScaleConsistencyMetricFailureReasonCode::MissingClosedGapObservations
    );
    assert_eq!(
        closed_gap_ratio.computation_failures[1].reason_code,
        ScaleConsistencyMetricFailureReasonCode::MissingClosedGapObservations
    );
}

#[test]
fn scale_consistency_metric_delta_calculator_preserves_deterministic_output_ordering() {
    let request = ScaleConsistencyMetricDeltaBatchRequest {
        metric_specifications: vec![
            metric_spec("closed_gap_abs_tail_expected_shortfall_ratio"),
            metric_spec("returns_distribution_wasserstein_distance"),
            metric_spec_with_lags("volatility_abs_return_acf_distribution_distance", &[2, 1]),
        ],
        scenarios: vec![
            build_scenario_input("scenario-b", "regime-b", "two_hour"),
            build_scenario_input("scenario-a", "regime-a", "hourly"),
        ],
    };

    let first_report =
        calculate_scale_consistency_metric_deltas(&request).expect("first run should succeed");
    let second_report =
        calculate_scale_consistency_metric_deltas(&request).expect("second run should succeed");

    assert_eq!(first_report, second_report);

    let scenario_ids = first_report
        .scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.scenario_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(scenario_ids, vec!["scenario-b", "scenario-a"]);

    let metric_ids = first_report.scenario_reports[0]
        .metric_deltas
        .iter()
        .map(|metric_delta| metric_delta.metric_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        metric_ids,
        vec![
            "closed_gap_abs_tail_expected_shortfall_ratio",
            "returns_distribution_wasserstein_distance",
            "volatility_abs_return_acf_distribution_distance",
        ]
    );
    assert_eq!(
        first_report.scenario_reports[0].metric_deltas[2]
            .metric_variant
            .as_deref(),
        Some("lag_grid_2_1")
    );
}

fn metric_spec(metric_id: &str) -> ScaleConsistencyMetricSpecification {
    ScaleConsistencyMetricSpecification {
        metric_id: metric_id.to_string(),
        metric_variant: None,
        lag_interval_counts: Vec::new(),
    }
}

fn metric_spec_with_lags(
    metric_id: &str,
    lag_interval_counts: &[usize],
) -> ScaleConsistencyMetricSpecification {
    ScaleConsistencyMetricSpecification {
        metric_id: metric_id.to_string(),
        metric_variant: None,
        lag_interval_counts: lag_interval_counts.to_vec(),
    }
}

fn build_scenario_input(
    scenario_id: &str,
    parameter_set_id: &str,
    horizon_label: &str,
) -> ScaleConsistencyMetricScenarioInput {
    let fine_path = build_fine_path(
        0.5,
        &[
            (0.1, 0.2, 1, 2, 0.2, -0.1, 0.5, 0.0, 0.0, 0.5),
            (0.3, 0.4, 3, 4, 0.1, -0.2, 0.5, 0.0, 0.0, 0.5),
            (0.5, 0.6, 5, 6, 0.2, -0.3, -0.5, 1.0, 1.0, 1.5),
            (0.7, 0.8, 7, 8, 0.3, -0.4, -0.5, 1.0, 1.0, 1.5),
            (0.9, 1.0, 9, 10, 0.4, -0.5, -3.5, 3.0, 3.0, 2.5),
            (1.1, 1.2, 11, 12, 0.5, -0.6, -3.5, 3.0, 3.0, 2.5),
        ],
    );
    let aligned_fine_path = aggregate_fine_risk_path_to_coarse_intervals(
        &fine_path,
        1.0,
        janiform_validation::ScaleConsistencyFinePathPartialWindowPolicy::Reject,
    )
    .expect("fine path should align");

    let coarse_path = build_coarse_path(
        1.0,
        &[
            (0.4, 0.6, 4, 6, 0.4, -0.3, 1.0, 0.0, 0.0, 1.0),
            (1.2, 1.4, 12, 14, 0.6, -0.7, 1.0, 1.0, 1.0, 2.0),
            (2.0, 2.2, 20, 22, 0.8, -0.9, -3.0, 4.0, 5.0, 4.0),
        ],
    );

    ScaleConsistencyMetricScenarioInput {
        scenario_id: scenario_id.to_string(),
        parameter_set_id: parameter_set_id.to_string(),
        horizon_label: horizon_label.to_string(),
        coarse_path,
        aligned_fine_path,
    }
}

fn build_sparse_scenario_input(
    scenario_id: &str,
    parameter_set_id: &str,
    horizon_label: &str,
) -> ScaleConsistencyMetricScenarioInput {
    let fine_path = build_fine_path(
        0.25,
        &[
            (0.1, 0.1, 1, 1, 0.0, 0.0, 0.1, 0.0, 0.0, 0.1),
            (0.1, 0.1, 1, 1, 0.0, 0.0, 0.1, 0.0, 0.0, 0.1),
        ],
    );
    let aligned_fine_path = aggregate_fine_risk_path_to_coarse_intervals(
        &fine_path,
        0.5,
        janiform_validation::ScaleConsistencyFinePathPartialWindowPolicy::Reject,
    )
    .expect("single interval fine path should align");

    let coarse_path = build_coarse_path(0.5, &[(0.1, 0.1, 1, 1, 0.0, 0.0, 0.2, 0.0, 0.0, 0.2)]);

    ScaleConsistencyMetricScenarioInput {
        scenario_id: scenario_id.to_string(),
        parameter_set_id: parameter_set_id.to_string(),
        horizon_label: horizon_label.to_string(),
        coarse_path,
        aligned_fine_path,
    }
}

fn build_coarse_path(
    interval_delta: f64,
    records: &[(f64, f64, u64, u64, f64, f64, f64, f64, f64, f64)],
) -> SufficientStatisticsRiskPath {
    build_path(29, interval_delta, records)
}

fn build_fine_path(
    interval_delta: f64,
    records: &[(f64, f64, u64, u64, f64, f64, f64, f64, f64, f64)],
) -> SufficientStatisticsRiskPath {
    build_path(31, interval_delta, records)
}

fn build_path(
    seed: u64,
    interval_delta: f64,
    records: &[(f64, f64, u64, u64, f64, f64, f64, f64, f64, f64)],
) -> SufficientStatisticsRiskPath {
    let records = records
        .iter()
        .enumerate()
        .map(|(interval_index, record)| build_record(interval_index, *record))
        .collect::<Vec<_>>();

    SufficientStatisticsRiskPath::try_new(
        seed,
        interval_delta,
        records.len(),
        build_state(),
        build_state(),
        records,
    )
    .expect("fixture path should validate")
}

fn build_record(
    interval_index: usize,
    record: (f64, f64, u64, u64, f64, f64, f64, f64, f64, f64),
) -> SufficientStatisticsRiskPathRecord {
    let (
        a_plus,
        a_minus,
        n_plus,
        n_minus,
        buy_side_return,
        sell_side_return,
        trading_time_return,
        open_jump_return,
        closed_gap_return,
        total_return,
    ) = record;

    let sufficient_statistics =
        SufficientStatisticsIntervalStatistics::try_new(a_plus, a_minus, n_plus, n_minus)
            .expect("interval statistics should validate");
    let return_components = SufficientStatisticsRiskReturnComponents::try_new(
        buy_side_return,
        sell_side_return,
        trading_time_return,
        open_jump_return,
        closed_gap_return,
        total_return,
    )
    .expect("return components should validate");

    SufficientStatisticsRiskPathRecord::try_new(
        interval_index,
        sufficient_statistics,
        return_components,
        total_return,
        total_return,
        total_return,
        0.0,
        0.0,
        total_return,
    )
    .expect("risk path record should validate")
}

fn build_state() -> HawkesExcitationState {
    HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    }
}

fn assert_close(left: f64, right: f64) {
    let absolute_difference = (left - right).abs();
    assert!(
        absolute_difference <= 1e-12,
        "expected {left} to equal {right} within tolerance, diff={absolute_difference}"
    );
}
