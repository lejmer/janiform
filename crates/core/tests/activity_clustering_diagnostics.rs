use janiform_core::verification::{
    ActivityClusteringBlockBootstrapConfiguration, ActivityClusteringDatasetInput,
    ActivityClusteringDiagnosticsError, ActivityClusteringDiagnosticsRequest,
    ActivityClusteringHorizonBucket, ActivityClusteringMetricSampleSufficiencyPolicy,
    ActivityClusteringMetricStatus, ActivityClusteringReturnObservation,
    ActivityClusteringSampleSufficiencyPolicy, ActivityClusteringThresholdPolicy,
    run_activity_clustering_diagnostics,
};

#[test]
fn activity_clustering_metrics_match_controlled_fixture_expectations() {
    let request = build_request(
        vec![ActivityClusteringHorizonBucket {
            horizon_label: "10n".to_string(),
            horizon_duration_nanos: 10,
        }],
        permissive_sample_sufficiency_policy(),
        4_242,
    );

    let report = run_activity_clustering_diagnostics(request)
        .expect("activity clustering diagnostics should succeed for controlled fixture");
    assert_eq!(report.horizons.len(), 1);

    let horizon_report = &report.horizons[0];
    let burst_metric = &horizon_report.event_burst_duration_quantile_ratio;
    let interarrival_metric = &horizon_report.event_interarrival_expected_shortfall_ratio;
    let volatility_metric = &horizon_report.volatility_abs_return_lag1_autocorrelation;

    assert_eq!(
        burst_metric.status,
        ActivityClusteringMetricStatus::Available
    );
    assert_eq!(
        interarrival_metric.status,
        ActivityClusteringMetricStatus::Available
    );
    assert_eq!(
        volatility_metric.status,
        ActivityClusteringMetricStatus::Available
    );

    let observed_burst_value = burst_metric
        .empirical
        .value
        .expect("burst metric value should be available");
    let observed_interarrival_value = interarrival_metric
        .empirical
        .value
        .expect("interarrival metric value should be available");
    let observed_volatility_value = volatility_metric
        .empirical
        .value
        .expect("volatility metric value should be available");

    assert!(
        (observed_burst_value - 1.4).abs() <= 1e-12,
        "unexpected burst-duration quantile ratio",
    );
    assert!(
        (observed_interarrival_value - 1.170_212_765_957_447_3).abs() <= 1e-12,
        "unexpected interarrival expected-shortfall ratio",
    );
    assert!(
        (observed_volatility_value + 0.9).abs() <= 1e-12,
        "unexpected lag-1 absolute-return autocorrelation",
    );
}

#[test]
fn activity_clustering_horizon_bucket_order_and_interval_scaling_are_preserved() {
    let request = build_request(
        vec![
            ActivityClusteringHorizonBucket {
                horizon_label: "10n".to_string(),
                horizon_duration_nanos: 10,
            },
            ActivityClusteringHorizonBucket {
                horizon_label: "25n".to_string(),
                horizon_duration_nanos: 25,
            },
        ],
        permissive_sample_sufficiency_policy(),
        4_242,
    );

    let report = run_activity_clustering_diagnostics(request)
        .expect("activity clustering diagnostics should succeed for multi-horizon fixture");
    assert_eq!(report.horizons.len(), 2);
    assert_eq!(report.horizons[0].horizon_label, "10n");
    assert_eq!(report.horizons[1].horizon_label, "25n");

    assert_eq!(
        report.horizons[0]
            .event_burst_duration_quantile_ratio
            .empirical
            .sample_coverage
            .horizon_interval_count,
        10
    );
    assert_eq!(
        report.horizons[1]
            .event_burst_duration_quantile_ratio
            .empirical
            .sample_coverage
            .horizon_interval_count,
        4
    );
}

#[test]
fn activity_clustering_rejects_duplicate_horizon_labels() {
    let request = build_request(
        vec![
            ActivityClusteringHorizonBucket {
                horizon_label: "duplicate".to_string(),
                horizon_duration_nanos: 10,
            },
            ActivityClusteringHorizonBucket {
                horizon_label: "duplicate".to_string(),
                horizon_duration_nanos: 20,
            },
        ],
        permissive_sample_sufficiency_policy(),
        4_242,
    );

    let error = run_activity_clustering_diagnostics(request)
        .expect_err("duplicate horizon labels should fail request validation");

    match error {
        ActivityClusteringDiagnosticsError::DuplicateHorizonLabel { horizon_label } => {
            assert_eq!(horizon_label, "duplicate");
        }
        other => panic!("expected DuplicateHorizonLabel error, found {other:?}"),
    }
}

#[test]
fn activity_clustering_report_is_deterministic_on_fixed_fixture() {
    let request = build_request(
        vec![ActivityClusteringHorizonBucket {
            horizon_label: "10n".to_string(),
            horizon_duration_nanos: 10,
        }],
        permissive_sample_sufficiency_policy(),
        7_777,
    );

    let first_report = run_activity_clustering_diagnostics(request.clone())
        .expect("first deterministic run should succeed");
    let second_report = run_activity_clustering_diagnostics(request)
        .expect("second deterministic run should succeed");

    assert_eq!(first_report, second_report);

    let first_payload =
        serde_json::to_string(&first_report).expect("first report should serialize");
    let second_payload =
        serde_json::to_string(&second_report).expect("second report should serialize");
    assert_eq!(first_payload, second_payload);
}

#[test]
fn activity_clustering_bootstrap_is_reproducible_and_sparse_horizon_is_insufficient() {
    let request = build_request(
        vec![
            ActivityClusteringHorizonBucket {
                horizon_label: "20n_dense".to_string(),
                horizon_duration_nanos: 20,
            },
            ActivityClusteringHorizonBucket {
                horizon_label: "5n_sparse".to_string(),
                horizon_duration_nanos: 5,
            },
        ],
        coverage_gated_sample_sufficiency_policy(),
        1_337,
    );

    let first_report = run_activity_clustering_diagnostics(request.clone())
        .expect("first sparse-horizon run should succeed");
    let second_report = run_activity_clustering_diagnostics(request)
        .expect("second sparse-horizon run should succeed");

    assert_eq!(first_report, second_report);

    let dense_horizon_report = &first_report.horizons[0];
    let sparse_horizon_report = &first_report.horizons[1];

    let dense_burst_metric = &dense_horizon_report.event_burst_duration_quantile_ratio;
    let dense_burst_confidence_interval = dense_burst_metric
        .empirical
        .confidence_interval
        .expect("dense horizon should include confidence interval");
    assert!(
        dense_burst_confidence_interval.lower_bound
            <= dense_burst_metric
                .empirical
                .value
                .expect("dense horizon should include point estimate")
            && dense_burst_confidence_interval.upper_bound
                >= dense_burst_metric
                    .empirical
                    .value
                    .expect("dense horizon should include point estimate"),
        "point estimate should be inside confidence interval",
    );

    assert_eq!(
        sparse_horizon_report
            .event_burst_duration_quantile_ratio
            .status,
        ActivityClusteringMetricStatus::InsufficientData
    );
    assert_eq!(
        sparse_horizon_report
            .event_interarrival_expected_shortfall_ratio
            .status,
        ActivityClusteringMetricStatus::InsufficientData
    );
    assert_eq!(
        sparse_horizon_report
            .volatility_abs_return_lag1_autocorrelation
            .status,
        ActivityClusteringMetricStatus::InsufficientData
    );
}

fn build_request(
    horizon_buckets: Vec<ActivityClusteringHorizonBucket>,
    sample_sufficiency_policy: ActivityClusteringSampleSufficiencyPolicy,
    random_seed: u64,
) -> ActivityClusteringDiagnosticsRequest {
    ActivityClusteringDiagnosticsRequest {
        analysis_start_utc_nanos: 0,
        analysis_end_utc_nanos: 100,
        empirical_dataset: build_dataset_input("empirical"),
        simulated_dataset: build_dataset_input("simulated"),
        horizon_buckets,
        bootstrap_configuration: ActivityClusteringBlockBootstrapConfiguration {
            confidence_level: 0.95,
            replication_count: 300,
            block_length_interval_count: 3,
            random_seed,
        },
        sample_sufficiency_policy,
        threshold_policy: ActivityClusteringThresholdPolicy::default(),
    }
}

fn build_dataset_input(dataset_id: &str) -> ActivityClusteringDatasetInput {
    ActivityClusteringDatasetInput {
        dataset_id: dataset_id.to_string(),
        event_timestamps_utc_nanos: vec![1, 5, 12, 18, 24, 41, 74, 82],
        return_observations: vec![
            ActivityClusteringReturnObservation {
                timestamp_utc_nanos: 2,
                return_increment: 1.0,
            },
            ActivityClusteringReturnObservation {
                timestamp_utc_nanos: 12,
                return_increment: -2.0,
            },
            ActivityClusteringReturnObservation {
                timestamp_utc_nanos: 22,
                return_increment: 1.0,
            },
            ActivityClusteringReturnObservation {
                timestamp_utc_nanos: 32,
                return_increment: -2.0,
            },
            ActivityClusteringReturnObservation {
                timestamp_utc_nanos: 42,
                return_increment: 1.0,
            },
            ActivityClusteringReturnObservation {
                timestamp_utc_nanos: 52,
                return_increment: -2.0,
            },
            ActivityClusteringReturnObservation {
                timestamp_utc_nanos: 62,
                return_increment: 1.0,
            },
            ActivityClusteringReturnObservation {
                timestamp_utc_nanos: 72,
                return_increment: -2.0,
            },
            ActivityClusteringReturnObservation {
                timestamp_utc_nanos: 82,
                return_increment: 1.0,
            },
            ActivityClusteringReturnObservation {
                timestamp_utc_nanos: 92,
                return_increment: -2.0,
            },
        ],
    }
}

fn permissive_sample_sufficiency_policy() -> ActivityClusteringSampleSufficiencyPolicy {
    ActivityClusteringSampleSufficiencyPolicy {
        event_burst_duration_quantile_ratio: ActivityClusteringMetricSampleSufficiencyPolicy {
            minimum_sample_count: 1,
            minimum_effective_sample_count: 1,
            minimum_tail_observation_count: 0,
            minimum_horizon_coverage_ratio: 0.05,
        },
        event_interarrival_expected_shortfall_ratio:
            ActivityClusteringMetricSampleSufficiencyPolicy {
                minimum_sample_count: 2,
                minimum_effective_sample_count: 2,
                minimum_tail_observation_count: 1,
                minimum_horizon_coverage_ratio: 0.05,
            },
        volatility_abs_return_lag1_autocorrelation:
            ActivityClusteringMetricSampleSufficiencyPolicy {
                minimum_sample_count: 2,
                minimum_effective_sample_count: 1,
                minimum_tail_observation_count: 0,
                minimum_horizon_coverage_ratio: 0.05,
            },
    }
}

fn coverage_gated_sample_sufficiency_policy() -> ActivityClusteringSampleSufficiencyPolicy {
    ActivityClusteringSampleSufficiencyPolicy {
        event_burst_duration_quantile_ratio: ActivityClusteringMetricSampleSufficiencyPolicy {
            minimum_sample_count: 1,
            minimum_effective_sample_count: 1,
            minimum_tail_observation_count: 0,
            minimum_horizon_coverage_ratio: 0.6,
        },
        event_interarrival_expected_shortfall_ratio:
            ActivityClusteringMetricSampleSufficiencyPolicy {
                minimum_sample_count: 2,
                minimum_effective_sample_count: 2,
                minimum_tail_observation_count: 1,
                minimum_horizon_coverage_ratio: 0.6,
            },
        volatility_abs_return_lag1_autocorrelation:
            ActivityClusteringMetricSampleSufficiencyPolicy {
                minimum_sample_count: 2,
                minimum_effective_sample_count: 1,
                minimum_tail_observation_count: 0,
                minimum_horizon_coverage_ratio: 0.6,
            },
    }
}
