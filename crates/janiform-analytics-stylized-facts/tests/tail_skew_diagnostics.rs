use janiform_analytics_stylized_facts::{
    TailSkewBlockBootstrapConfiguration, TailSkewDatasetInput, TailSkewDiagnosticsRequest,
    TailSkewHorizonBucket, TailSkewHorizonDiagnostics, TailSkewMetricComparison,
    TailSkewMetricSampleSufficiencyPolicy, TailSkewMetricStatus, TailSkewReturnObservation,
    TailSkewSampleSufficiencyPolicy, run_tail_skew_diagnostics,
};

#[test]
fn tail_skew_metrics_match_symmetric_heavy_tail_fixture() {
    let mut symmetric_heavy_tail_values: Vec<f64> = Vec::new();
    for _ in 0..80 {
        symmetric_heavy_tail_values
            .extend_from_slice(&[-4.0, -2.0, -1.0, -0.5, 0.5, 1.0, 2.0, 4.0]);
    }

    let request = build_request(
        &symmetric_heavy_tail_values,
        &symmetric_heavy_tail_values,
        vec![TailSkewHorizonBucket {
            horizon_label: "10n".to_string(),
            horizon_duration_nanos: 10,
        }],
        permissive_sample_sufficiency_policy(),
        11_221,
    );

    let report = run_tail_skew_diagnostics(request)
        .expect("tail and skew diagnostics should succeed on symmetric heavy-tail fixture");

    assert_eq!(report.horizons.len(), 1);

    let horizon_report = &report.horizons[0];

    let abs_quantile_ratio =
        metric_by_id(horizon_report, "returns_abs_quantile_shape_ratio_q99_q95");
    let left_tail_ratio = metric_by_id(
        horizon_report,
        "returns_left_tail_expected_shortfall_to_var_ratio",
    );
    let bowley_skewness = metric_by_id(horizon_report, "returns_bowley_skewness_quantile");
    let signed_tail_imbalance = metric_by_id(
        horizon_report,
        "returns_signed_tail_expected_shortfall_imbalance",
    );
    let l_moment_lambda2 = metric_by_id(horizon_report, "returns_l_moment_lambda2");
    let central_variance = metric_by_id(horizon_report, "returns_central_variance");
    let central_skewness = metric_by_id(horizon_report, "returns_central_skewness");
    let central_excess_kurtosis = metric_by_id(horizon_report, "returns_central_excess_kurtosis");

    assert_eq!(abs_quantile_ratio.status, TailSkewMetricStatus::Available);
    assert_eq!(left_tail_ratio.status, TailSkewMetricStatus::Available);
    assert_eq!(bowley_skewness.status, TailSkewMetricStatus::Available);
    assert_eq!(
        signed_tail_imbalance.status,
        TailSkewMetricStatus::Available
    );
    assert_eq!(l_moment_lambda2.status, TailSkewMetricStatus::Available);
    assert_eq!(central_variance.status, TailSkewMetricStatus::Available);
    assert_eq!(central_skewness.status, TailSkewMetricStatus::Available);
    assert_eq!(
        central_excess_kurtosis.status,
        TailSkewMetricStatus::Available
    );

    assert!(
        abs_quantile_ratio
            .empirical
            .value
            .expect("abs quantile ratio should be available")
            >= 1.0,
    );
    assert!(
        left_tail_ratio
            .empirical
            .value
            .expect("left-tail ratio should be available")
            >= 1.0,
    );
    assert!(
        bowley_skewness
            .empirical
            .value
            .expect("bowley skewness should be available")
            .abs()
            <= 1e-12,
    );
    assert!(
        signed_tail_imbalance
            .empirical
            .value
            .expect("signed-tail imbalance should be available")
            .abs()
            <= 1e-12,
    );
    assert!(
        l_moment_lambda2
            .empirical
            .value
            .expect("l-moment lambda2 should be available")
            > 0.0,
    );
    assert!(
        central_variance
            .empirical
            .value
            .expect("central variance should be available")
            > 0.0,
    );
    assert!(
        central_skewness
            .empirical
            .value
            .expect("central skewness should be available")
            .abs()
            <= 1e-12,
    );
    assert!(
        central_excess_kurtosis
            .empirical
            .value
            .expect("central excess kurtosis should be available")
            .is_finite(),
    );

    let confidence_interval = abs_quantile_ratio
        .empirical
        .confidence_interval
        .expect("available metrics should carry confidence intervals");
    let point_estimate = abs_quantile_ratio
        .empirical
        .value
        .expect("point estimate should exist");
    assert!(
        confidence_interval.lower_bound <= point_estimate
            && confidence_interval.upper_bound >= point_estimate,
        "point estimate should be contained in confidence interval",
    );
}

#[test]
fn tail_skew_sparse_horizon_is_marked_insufficient_data() {
    let sparse_fixture_values: Vec<f64> =
        vec![-2.0, 1.0, -1.0, 2.0, -1.5, 1.5, -0.5, 0.5, -2.5, 2.5];

    let request = build_request(
        &sparse_fixture_values,
        &sparse_fixture_values,
        vec![
            TailSkewHorizonBucket {
                horizon_label: "10n_dense".to_string(),
                horizon_duration_nanos: 10,
            },
            TailSkewHorizonBucket {
                horizon_label: "5n_sparse".to_string(),
                horizon_duration_nanos: 5,
            },
        ],
        coverage_gated_sample_sufficiency_policy(),
        4_001,
    );

    let report = run_tail_skew_diagnostics(request)
        .expect("tail and skew diagnostics should succeed on sparse fixture");

    assert_eq!(report.horizons.len(), 2);

    let dense_horizon = &report.horizons[0];
    let sparse_horizon = &report.horizons[1];

    assert_eq!(
        metric_by_id(dense_horizon, "returns_central_variance").status,
        TailSkewMetricStatus::Available,
    );

    for metric in &sparse_horizon.metrics {
        assert_eq!(
            metric.status,
            TailSkewMetricStatus::InsufficientData,
            "metric {} should be insufficient for sparse horizon",
            metric.metric_id,
        );
    }
}

#[test]
fn tail_skew_report_is_deterministic_on_fixed_fixture() {
    let mut values: Vec<f64> = Vec::new();
    for _ in 0..60 {
        values.extend_from_slice(&[-3.0, -1.0, -0.25, 0.25, 1.0, 3.0]);
    }

    let request = build_request(
        &values,
        &values,
        vec![TailSkewHorizonBucket {
            horizon_label: "10n".to_string(),
            horizon_duration_nanos: 10,
        }],
        permissive_sample_sufficiency_policy(),
        9_999,
    );

    let first_report =
        run_tail_skew_diagnostics(request.clone()).expect("first run should succeed");
    let second_report = run_tail_skew_diagnostics(request).expect("second run should succeed");

    assert_eq!(first_report, second_report);

    let first_payload =
        serde_json::to_string(&first_report).expect("first payload should serialize");
    let second_payload =
        serde_json::to_string(&second_report).expect("second payload should serialize");
    assert_eq!(first_payload, second_payload);
}

#[test]
fn tail_skew_robust_primary_metrics_are_less_outlier_sensitive_than_classical_moments() {
    let mut baseline_values: Vec<f64> = Vec::new();
    for _ in 0..100 {
        baseline_values.extend_from_slice(&[-3.0, -2.0, -1.0, -0.5, 0.5, 1.0, 2.0, 3.0]);
    }

    let mut stressed_values = baseline_values.clone();
    stressed_values.extend_from_slice(&[60.0, 70.0, 80.0, 90.0, 100.0]);

    let request = build_request(
        &baseline_values,
        &stressed_values,
        vec![TailSkewHorizonBucket {
            horizon_label: "10n".to_string(),
            horizon_duration_nanos: 10,
        }],
        permissive_sample_sufficiency_policy(),
        7_313,
    );

    let report = run_tail_skew_diagnostics(request)
        .expect("tail and skew diagnostics should succeed on outlier stress fixture");

    let horizon_report = &report.horizons[0];

    let bowley_delta = metric_by_id(horizon_report, "returns_bowley_skewness_quantile")
        .absolute_delta
        .expect("bowley delta should be available");
    let classical_skew_delta = metric_by_id(horizon_report, "returns_central_skewness")
        .absolute_delta
        .expect("classical skew delta should be available");

    let abs_quantile_delta =
        metric_by_id(horizon_report, "returns_abs_quantile_shape_ratio_q99_q95")
            .absolute_delta
            .expect("abs quantile delta should be available");
    let classical_kurtosis_delta = metric_by_id(horizon_report, "returns_central_excess_kurtosis")
        .absolute_delta
        .expect("classical kurtosis delta should be available");

    assert!(
        classical_skew_delta > bowley_delta * 2.0,
        "classical skewness should react more strongly than bowley skewness under outlier stress",
    );
    assert!(
        classical_kurtosis_delta > abs_quantile_delta * 2.0,
        "classical excess kurtosis should react more strongly than quantile-shape ratio under outlier stress",
    );
}

fn metric_by_id<'a>(
    horizon_report: &'a TailSkewHorizonDiagnostics,
    metric_id: &str,
) -> &'a TailSkewMetricComparison {
    horizon_report
        .metrics
        .iter()
        .find(|metric| metric.metric_id == metric_id)
        .unwrap_or_else(|| panic!("missing metric `{metric_id}`"))
}

fn build_request(
    empirical_values: &[f64],
    simulated_values: &[f64],
    horizon_buckets: Vec<TailSkewHorizonBucket>,
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
    random_seed: u64,
) -> TailSkewDiagnosticsRequest {
    let max_length = empirical_values.len().max(simulated_values.len()) as i64;

    TailSkewDiagnosticsRequest {
        analysis_start_utc_nanos: 0,
        analysis_end_utc_nanos: max_length * 10 + 10,
        empirical_dataset: build_dataset_input("empirical", empirical_values),
        simulated_dataset: build_dataset_input("simulated", simulated_values),
        horizon_buckets,
        bootstrap_configuration: TailSkewBlockBootstrapConfiguration {
            confidence_level: 0.95,
            replication_count: 250,
            block_length_interval_count: 4,
            random_seed,
        },
        sample_sufficiency_policy,
    }
}

fn build_dataset_input(dataset_id: &str, values: &[f64]) -> TailSkewDatasetInput {
    TailSkewDatasetInput {
        dataset_id: dataset_id.to_string(),
        return_observations: values
            .iter()
            .enumerate()
            .map(|(position, value)| TailSkewReturnObservation {
                timestamp_utc_nanos: position as i64 * 10 + 1,
                return_increment: *value,
            })
            .collect(),
    }
}

fn permissive_sample_sufficiency_policy() -> TailSkewSampleSufficiencyPolicy {
    let non_tail_metric_policy = TailSkewMetricSampleSufficiencyPolicy {
        minimum_sample_count: 10,
        minimum_effective_sample_count: 8,
        minimum_tail_observation_count: 0,
        minimum_horizon_coverage_ratio: 0.3,
    };
    let tail_metric_policy = TailSkewMetricSampleSufficiencyPolicy {
        minimum_sample_count: 10,
        minimum_effective_sample_count: 8,
        minimum_tail_observation_count: 2,
        minimum_horizon_coverage_ratio: 0.3,
    };

    TailSkewSampleSufficiencyPolicy {
        returns_abs_quantile_shape_ratio_q99_q95: non_tail_metric_policy,
        returns_left_tail_expected_shortfall_to_var_ratio: tail_metric_policy,
        returns_bowley_skewness_quantile: non_tail_metric_policy,
        returns_signed_tail_expected_shortfall_imbalance: tail_metric_policy,
        returns_l_moment_lambda2: non_tail_metric_policy,
        returns_l_moment_tau3: non_tail_metric_policy,
        returns_central_variance: non_tail_metric_policy,
        returns_central_skewness: non_tail_metric_policy,
        returns_central_excess_kurtosis: non_tail_metric_policy,
    }
}

fn coverage_gated_sample_sufficiency_policy() -> TailSkewSampleSufficiencyPolicy {
    let non_tail_metric_policy = TailSkewMetricSampleSufficiencyPolicy {
        minimum_sample_count: 8,
        minimum_effective_sample_count: 6,
        minimum_tail_observation_count: 0,
        minimum_horizon_coverage_ratio: 0.9,
    };
    let tail_metric_policy = TailSkewMetricSampleSufficiencyPolicy {
        minimum_sample_count: 8,
        minimum_effective_sample_count: 6,
        minimum_tail_observation_count: 1,
        minimum_horizon_coverage_ratio: 0.9,
    };

    TailSkewSampleSufficiencyPolicy {
        returns_abs_quantile_shape_ratio_q99_q95: non_tail_metric_policy,
        returns_left_tail_expected_shortfall_to_var_ratio: tail_metric_policy,
        returns_bowley_skewness_quantile: non_tail_metric_policy,
        returns_signed_tail_expected_shortfall_imbalance: tail_metric_policy,
        returns_l_moment_lambda2: non_tail_metric_policy,
        returns_l_moment_tau3: non_tail_metric_policy,
        returns_central_variance: non_tail_metric_policy,
        returns_central_skewness: non_tail_metric_policy,
        returns_central_excess_kurtosis: non_tail_metric_policy,
    }
}
