use janiform_validation::{
    VolatilityPersistenceBlockBootstrapConfiguration, VolatilityPersistenceDatasetInput,
    VolatilityPersistenceDiagnosticsError, VolatilityPersistenceDiagnosticsRequest,
    VolatilityPersistenceHorizonBucket, VolatilityPersistenceLagComparison,
    VolatilityPersistenceLagSetting, VolatilityPersistenceMetricSampleSufficiencyPolicy,
    VolatilityPersistenceMetricStatus, VolatilityPersistenceReturnObservation,
    VolatilityPersistenceSampleSufficiencyPolicy, VolatilityPersistenceThresholdPolicy,
    run_volatility_persistence_diagnostics,
};

#[test]
fn volatility_persistence_autocorrelation_matches_controlled_fixture() {
    let request = build_request(
        vec![VolatilityPersistenceHorizonBucket {
            horizon_label: "10n".to_string(),
            horizon_duration_nanos: 10,
        }],
        vec![
            VolatilityPersistenceLagSetting {
                lag_label: "lag_1".to_string(),
                lag_interval_count: 1,
            },
            VolatilityPersistenceLagSetting {
                lag_label: "lag_2".to_string(),
                lag_interval_count: 2,
            },
            VolatilityPersistenceLagSetting {
                lag_label: "lag_3".to_string(),
                lag_interval_count: 3,
            },
        ],
        permissive_sample_sufficiency_policy(),
        4_242,
    );

    let report = run_volatility_persistence_diagnostics(request)
        .expect("volatility persistence diagnostics should succeed for controlled fixture");
    assert_eq!(report.horizons.len(), 1);
    assert_eq!(
        report.metric_catalog_metric_id,
        "volatility_abs_return_acf_distribution_distance"
    );

    let horizon_report = &report.horizons[0];
    let lag_one_comparison =
        lag_comparison_by_label(horizon_report.lag_comparisons.as_slice(), "lag_1");
    let lag_two_comparison =
        lag_comparison_by_label(horizon_report.lag_comparisons.as_slice(), "lag_2");
    let lag_three_comparison =
        lag_comparison_by_label(horizon_report.lag_comparisons.as_slice(), "lag_3");

    assert_eq!(
        lag_one_comparison.status,
        VolatilityPersistenceMetricStatus::Available
    );
    assert_eq!(
        lag_two_comparison.status,
        VolatilityPersistenceMetricStatus::Available
    );
    assert_eq!(
        lag_three_comparison.status,
        VolatilityPersistenceMetricStatus::Available
    );

    let lag_one_value = lag_one_comparison
        .empirical
        .value
        .expect("lag one autocorrelation should be available");
    let lag_two_value = lag_two_comparison
        .empirical
        .value
        .expect("lag two autocorrelation should be available");
    let lag_three_value = lag_three_comparison
        .empirical
        .value
        .expect("lag three autocorrelation should be available");

    assert!(
        (lag_one_value + 0.9).abs() <= 1e-12,
        "unexpected lag-1 autocorrelation",
    );
    assert!(
        (lag_two_value - 0.8).abs() <= 1e-12,
        "unexpected lag-2 autocorrelation",
    );
    assert!(
        (lag_three_value + 0.7).abs() <= 1e-12,
        "unexpected lag-3 autocorrelation",
    );

    assert_eq!(
        lag_one_comparison.absolute_delta,
        Some(0.0),
        "simulated and empirical fixtures are identical",
    );
    assert_eq!(
        lag_one_comparison
            .threshold_context
            .absolute_delta_exceeds_threshold,
        Some(false),
    );
}

#[test]
fn volatility_persistence_rejects_invalid_horizon_duration() {
    let request = build_request(
        vec![VolatilityPersistenceHorizonBucket {
            horizon_label: "invalid".to_string(),
            horizon_duration_nanos: 0,
        }],
        vec![VolatilityPersistenceLagSetting {
            lag_label: "lag_1".to_string(),
            lag_interval_count: 1,
        }],
        permissive_sample_sufficiency_policy(),
        4_242,
    );

    let error = run_volatility_persistence_diagnostics(request)
        .expect_err("zero horizon duration should fail request validation");
    match error {
        VolatilityPersistenceDiagnosticsError::InvalidHorizonDuration {
            horizon_label,
            horizon_duration_nanos,
        } => {
            assert_eq!(horizon_label, "invalid");
            assert_eq!(horizon_duration_nanos, 0);
        }
        other => panic!("expected InvalidHorizonDuration error, found {other:?}"),
    }
}

#[test]
fn volatility_persistence_rejects_zero_lag_interval_count() {
    let request = build_request(
        vec![VolatilityPersistenceHorizonBucket {
            horizon_label: "10n".to_string(),
            horizon_duration_nanos: 10,
        }],
        vec![VolatilityPersistenceLagSetting {
            lag_label: "lag_0".to_string(),
            lag_interval_count: 0,
        }],
        permissive_sample_sufficiency_policy(),
        4_242,
    );

    let error = run_volatility_persistence_diagnostics(request)
        .expect_err("zero lag should fail request validation");
    match error {
        VolatilityPersistenceDiagnosticsError::InvalidLagIntervalCount {
            lag_label,
            lag_interval_count,
        } => {
            assert_eq!(lag_label, "lag_0");
            assert_eq!(lag_interval_count, 0);
        }
        other => panic!("expected InvalidLagIntervalCount error, found {other:?}"),
    }
}

#[test]
fn volatility_persistence_report_is_deterministic_on_fixed_fixture() {
    let request = build_request(
        vec![VolatilityPersistenceHorizonBucket {
            horizon_label: "10n".to_string(),
            horizon_duration_nanos: 10,
        }],
        vec![
            VolatilityPersistenceLagSetting {
                lag_label: "lag_1".to_string(),
                lag_interval_count: 1,
            },
            VolatilityPersistenceLagSetting {
                lag_label: "lag_2".to_string(),
                lag_interval_count: 2,
            },
        ],
        permissive_sample_sufficiency_policy(),
        7_777,
    );

    let first_report = run_volatility_persistence_diagnostics(request.clone())
        .expect("first deterministic run should succeed");
    let second_report = run_volatility_persistence_diagnostics(request)
        .expect("second deterministic run should succeed");

    assert_eq!(first_report, second_report);

    let first_payload =
        serde_json::to_string(&first_report).expect("first report should serialize");
    let second_payload =
        serde_json::to_string(&second_report).expect("second report should serialize");
    assert_eq!(first_payload, second_payload);
}

#[test]
fn volatility_persistence_sparse_horizon_emits_insufficient_data_with_complete_metadata() {
    let sample_sufficiency_policy = coverage_gated_sample_sufficiency_policy();
    let request = build_request(
        vec![
            VolatilityPersistenceHorizonBucket {
                horizon_label: "20n_dense".to_string(),
                horizon_duration_nanos: 20,
            },
            VolatilityPersistenceHorizonBucket {
                horizon_label: "5n_sparse".to_string(),
                horizon_duration_nanos: 5,
            },
        ],
        vec![
            VolatilityPersistenceLagSetting {
                lag_label: "lag_1".to_string(),
                lag_interval_count: 1,
            },
            VolatilityPersistenceLagSetting {
                lag_label: "lag_2".to_string(),
                lag_interval_count: 2,
            },
        ],
        sample_sufficiency_policy,
        1_337,
    );

    let report = run_volatility_persistence_diagnostics(request)
        .expect("volatility persistence diagnostics should succeed for sparse fixture");
    assert_eq!(report.horizons.len(), 2);

    let dense_horizon_report = &report.horizons[0];
    let sparse_horizon_report = &report.horizons[1];

    assert!(
        dense_horizon_report
            .lag_comparisons
            .iter()
            .all(|comparison| comparison.status == VolatilityPersistenceMetricStatus::Available),
        "dense horizon should be available across lag settings",
    );

    for lag_comparison in &sparse_horizon_report.lag_comparisons {
        assert_eq!(
            lag_comparison.status,
            VolatilityPersistenceMetricStatus::InsufficientData
        );
        assert_eq!(
            lag_comparison.empirical.status,
            VolatilityPersistenceMetricStatus::InsufficientData
        );
        assert_eq!(
            lag_comparison.simulated.status,
            VolatilityPersistenceMetricStatus::InsufficientData
        );
        assert!(lag_comparison.empirical.confidence_interval.is_none());
        assert!(lag_comparison.simulated.confidence_interval.is_none());
        assert!(lag_comparison.absolute_delta.is_none());
        assert_eq!(
            lag_comparison
                .threshold_context
                .absolute_delta_exceeds_threshold,
            None,
        );
        assert_eq!(
            lag_comparison
                .uncertainty_context
                .empirical_interval_half_width,
            None,
        );
        assert_eq!(
            lag_comparison
                .uncertainty_context
                .simulated_interval_half_width,
            None,
        );
        assert_eq!(
            lag_comparison
                .uncertainty_context
                .pooled_interval_half_width,
            None,
        );
        assert_eq!(lag_comparison.uncertainty_context.standardized_delta, None);
        assert_eq!(
            lag_comparison
                .empirical
                .sample_coverage
                .horizon_interval_count,
            20
        );
        assert_eq!(
            lag_comparison
                .empirical
                .sample_coverage
                .covered_interval_count,
            10
        );
        assert_eq!(
            lag_comparison
                .empirical
                .required_minimums
                .minimum_horizon_coverage_ratio,
            sample_sufficiency_policy
                .lagged_abs_return_autocorrelation
                .minimum_horizon_coverage_ratio,
        );
    }
}

fn lag_comparison_by_label<'a>(
    lag_comparisons: &'a [VolatilityPersistenceLagComparison],
    lag_label: &str,
) -> &'a VolatilityPersistenceLagComparison {
    lag_comparisons
        .iter()
        .find(|lag_comparison| lag_comparison.lag_label == lag_label)
        .unwrap_or_else(|| panic!("missing lag comparison `{lag_label}`"))
}

fn build_request(
    horizon_buckets: Vec<VolatilityPersistenceHorizonBucket>,
    lag_settings: Vec<VolatilityPersistenceLagSetting>,
    sample_sufficiency_policy: VolatilityPersistenceSampleSufficiencyPolicy,
    random_seed: u64,
) -> VolatilityPersistenceDiagnosticsRequest {
    VolatilityPersistenceDiagnosticsRequest {
        analysis_start_utc_nanos: 0,
        analysis_end_utc_nanos: 100,
        empirical_dataset: build_dataset_input("empirical"),
        simulated_dataset: build_dataset_input("simulated"),
        horizon_buckets,
        lag_settings,
        bootstrap_configuration: VolatilityPersistenceBlockBootstrapConfiguration {
            confidence_level: 0.95,
            replication_count: 300,
            block_length_interval_count: 3,
            random_seed,
        },
        sample_sufficiency_policy,
        threshold_policy: VolatilityPersistenceThresholdPolicy::default(),
    }
}

fn build_dataset_input(dataset_id: &str) -> VolatilityPersistenceDatasetInput {
    VolatilityPersistenceDatasetInput {
        dataset_id: dataset_id.to_string(),
        return_observations: vec![
            VolatilityPersistenceReturnObservation {
                timestamp_utc_nanos: 2,
                return_increment: 1.0,
            },
            VolatilityPersistenceReturnObservation {
                timestamp_utc_nanos: 12,
                return_increment: -2.0,
            },
            VolatilityPersistenceReturnObservation {
                timestamp_utc_nanos: 22,
                return_increment: 1.0,
            },
            VolatilityPersistenceReturnObservation {
                timestamp_utc_nanos: 32,
                return_increment: -2.0,
            },
            VolatilityPersistenceReturnObservation {
                timestamp_utc_nanos: 42,
                return_increment: 1.0,
            },
            VolatilityPersistenceReturnObservation {
                timestamp_utc_nanos: 52,
                return_increment: -2.0,
            },
            VolatilityPersistenceReturnObservation {
                timestamp_utc_nanos: 62,
                return_increment: 1.0,
            },
            VolatilityPersistenceReturnObservation {
                timestamp_utc_nanos: 72,
                return_increment: -2.0,
            },
            VolatilityPersistenceReturnObservation {
                timestamp_utc_nanos: 82,
                return_increment: 1.0,
            },
            VolatilityPersistenceReturnObservation {
                timestamp_utc_nanos: 92,
                return_increment: -2.0,
            },
        ],
    }
}

fn permissive_sample_sufficiency_policy() -> VolatilityPersistenceSampleSufficiencyPolicy {
    VolatilityPersistenceSampleSufficiencyPolicy {
        lagged_abs_return_autocorrelation: VolatilityPersistenceMetricSampleSufficiencyPolicy {
            minimum_sample_count: 2,
            minimum_effective_sample_count: 1,
            minimum_tail_observation_count: 0,
            minimum_horizon_coverage_ratio: 0.05,
        },
    }
}

fn coverage_gated_sample_sufficiency_policy() -> VolatilityPersistenceSampleSufficiencyPolicy {
    VolatilityPersistenceSampleSufficiencyPolicy {
        lagged_abs_return_autocorrelation: VolatilityPersistenceMetricSampleSufficiencyPolicy {
            minimum_sample_count: 2,
            minimum_effective_sample_count: 1,
            minimum_tail_observation_count: 0,
            minimum_horizon_coverage_ratio: 0.6,
        },
    }
}
