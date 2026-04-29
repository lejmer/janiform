mod support;

use std::collections::BTreeMap;

use janiform_analytics_stylized_facts::{
    ActivityClusteringBlockBootstrapConfiguration, ActivityClusteringDatasetInput,
    ActivityClusteringDiagnosticsRequest, ActivityClusteringHorizonBucket,
    ActivityClusteringMetricSampleSufficiencyPolicy, ActivityClusteringReturnObservation,
    ActivityClusteringSampleSufficiencyPolicy, ActivityClusteringThresholdPolicy,
    GapBehaviorBlockBootstrapConfiguration, GapBehaviorClosedWindowRegime, GapBehaviorDatasetInput,
    GapBehaviorDiagnosticsRequest, GapBehaviorEventObservation, GapBehaviorHorizonBucket,
    GapBehaviorMarketState, GapBehaviorMetricSampleSufficiencyPolicy,
    GapBehaviorSampleSufficiencyPolicy, GapBehaviorScenarioInput, GapBehaviorThresholdPolicy,
    NewsSemanticsScenarioClass, OpenClosedGapDeterministicReplayMetadata,
    OpenClosedGapIssueTraceabilityMetadata, TailSkewBlockBootstrapConfiguration,
    TailSkewDatasetInput, TailSkewDiagnosticsRequest, TailSkewHorizonBucket,
    TailSkewMetricSampleSufficiencyPolicy, TailSkewReturnObservation,
    TailSkewSampleSufficiencyPolicy, VolatilityPersistenceBlockBootstrapConfiguration,
    VolatilityPersistenceDatasetInput, VolatilityPersistenceDiagnosticsRequest,
    VolatilityPersistenceHorizonBucket, VolatilityPersistenceLagSetting,
    VolatilityPersistenceMetricSampleSufficiencyPolicy, VolatilityPersistenceReturnObservation,
    VolatilityPersistenceSampleSufficiencyPolicy, VolatilityPersistenceThresholdPolicy,
    run_activity_clustering_diagnostics, run_gap_behavior_diagnostics, run_tail_skew_diagnostics,
    run_volatility_persistence_diagnostics,
};
use serde::Deserialize;

use support::stylized_fact_regression_fixture::{
    StylizedFactDiagnosticKind, StylizedFactExpectedConfidenceInterval,
    StylizedFactExpectedMetricRegressionEntry, StylizedFactExpectedMetricStatus,
    StylizedFactMetricRegressionCase, StylizedFactRegressionTolerancePolicy,
    StylizedFactRegressionTolerancePolicyKind,
    load_stylized_fact_metric_regression_fixture_from_manifest,
};

#[test]
fn stylized_fact_metric_regression_fixture_matches_reference_outputs() {
    let loaded_fixture = load_stylized_fact_metric_regression_fixture_from_manifest()
        .expect("stylized fact regression fixture should load from manifest");

    let tolerance_policies: BTreeMap<&str, &StylizedFactRegressionTolerancePolicy> = loaded_fixture
        .fixture_document
        .tolerance_policies
        .iter()
        .map(|policy| (policy.policy_id.as_str(), policy))
        .collect();

    let mut mismatch_diagnostics: Vec<String> = Vec::new();
    for fixture_case in &loaded_fixture.fixture_document.cases {
        let tolerance_policy = tolerance_policies
            .get(fixture_case.tolerance_policy_id.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "missing tolerance policy {} for fixture {}",
                    fixture_case.tolerance_policy_id, fixture_case.fixture_id
                )
            });

        mismatch_diagnostics.extend(compare_case_against_reference(
            fixture_case,
            tolerance_policy,
        ));
    }

    assert!(
        mismatch_diagnostics.is_empty(),
        "stylized fact regression mismatches:\n{}",
        mismatch_diagnostics.join("\n"),
    );
}

#[test]
fn stylized_fact_tolerance_policy_boundary_behavior_is_inclusive() {
    assert!(within_tolerance(1.0, 1.1, 0.1));
    assert!(within_tolerance(-0.2, -0.3, 0.1));
    assert!(!within_tolerance(1.0, 1.100_000_000_001, 0.1));
}

fn compare_case_against_reference(
    fixture_case: &StylizedFactMetricRegressionCase,
    tolerance_policy: &StylizedFactRegressionTolerancePolicy,
) -> Vec<String> {
    let observed_metrics = match collect_observed_metrics(fixture_case) {
        Ok(observed_metrics) => observed_metrics,
        Err(message) => {
            return vec![format!(
                "fixture_id={} diagnostic execution failed: {}",
                fixture_case.fixture_id, message
            )];
        }
    };

    let expected_metrics: BTreeMap<MetricKey, &StylizedFactExpectedMetricRegressionEntry> =
        fixture_case
            .expected_metrics
            .iter()
            .map(|expected_metric| (MetricKey::from_expected(expected_metric), expected_metric))
            .collect();

    let mut mismatches: Vec<String> = Vec::new();

    for missing_key in expected_metrics
        .keys()
        .filter(|expected_key| !observed_metrics.contains_key(*expected_key))
    {
        mismatches.push(format!(
            "fixture_id={} missing observed metric horizon={} metric_id={} metric_variant={:?}",
            fixture_case.fixture_id,
            missing_key.horizon_label,
            missing_key.metric_id,
            missing_key.metric_variant
        ));
    }

    for unexpected_key in observed_metrics
        .keys()
        .filter(|observed_key| !expected_metrics.contains_key(*observed_key))
    {
        mismatches.push(format!(
            "fixture_id={} unexpected observed metric horizon={} metric_id={} metric_variant={:?}",
            fixture_case.fixture_id,
            unexpected_key.horizon_label,
            unexpected_key.metric_id,
            unexpected_key.metric_variant
        ));
    }

    for (metric_key, expected_metric) in expected_metrics {
        let Some(observed_metric) = observed_metrics.get(&metric_key) else {
            continue;
        };

        if expected_metric.expected_status != observed_metric.status {
            mismatches.push(format!(
                "fixture_id={} horizon={} metric_id={} metric_variant={:?} status mismatch expected={:?} observed={:?} {}",
                fixture_case.fixture_id,
                metric_key.horizon_label,
                metric_key.metric_id,
                metric_key.metric_variant,
                expected_metric.expected_status,
                observed_metric.status,
                observed_metric.debug_context
            ));
            continue;
        }

        compare_optional_float(
            &mut mismatches,
            fixture_case.fixture_id.as_str(),
            &metric_key,
            "expected_empirical_value",
            expected_metric.expected_empirical_value,
            observed_metric.empirical_value,
            tolerance_policy.point_estimate_absolute_tolerance,
            observed_metric.debug_context.as_str(),
        );
        compare_optional_float(
            &mut mismatches,
            fixture_case.fixture_id.as_str(),
            &metric_key,
            "expected_simulated_value",
            expected_metric.expected_simulated_value,
            observed_metric.simulated_value,
            tolerance_policy.point_estimate_absolute_tolerance,
            observed_metric.debug_context.as_str(),
        );
        compare_optional_float(
            &mut mismatches,
            fixture_case.fixture_id.as_str(),
            &metric_key,
            "expected_absolute_delta",
            expected_metric.expected_absolute_delta,
            observed_metric.absolute_delta,
            tolerance_policy.absolute_delta_tolerance,
            observed_metric.debug_context.as_str(),
        );

        if tolerance_policy.require_confidence_intervals {
            compare_optional_confidence_interval(
                &mut mismatches,
                fixture_case.fixture_id.as_str(),
                &metric_key,
                "expected_empirical_confidence_interval",
                expected_metric.expected_empirical_confidence_interval,
                observed_metric.empirical_confidence_interval,
                tolerance_policy.confidence_interval_bound_absolute_tolerance,
                observed_metric.debug_context.as_str(),
            );
            compare_optional_confidence_interval(
                &mut mismatches,
                fixture_case.fixture_id.as_str(),
                &metric_key,
                "expected_simulated_confidence_interval",
                expected_metric.expected_simulated_confidence_interval,
                observed_metric.simulated_confidence_interval,
                tolerance_policy.confidence_interval_bound_absolute_tolerance,
                observed_metric.debug_context.as_str(),
            );
        } else if matches!(
            tolerance_policy.policy_kind,
            StylizedFactRegressionTolerancePolicyKind::DeterministicExactMatch
        ) {
            if expected_metric
                .expected_empirical_confidence_interval
                .is_some()
                || expected_metric
                    .expected_simulated_confidence_interval
                    .is_some()
            {
                mismatches.push(format!(
                    "fixture_id={} horizon={} metric_id={} metric_variant={:?} deterministic exact-match policy should not define expected confidence intervals",
                    fixture_case.fixture_id,
                    metric_key.horizon_label,
                    metric_key.metric_id,
                    metric_key.metric_variant
                ));
            }
        }
    }

    mismatches
}

fn collect_observed_metrics(
    fixture_case: &StylizedFactMetricRegressionCase,
) -> Result<BTreeMap<MetricKey, ObservedMetric>, String> {
    match fixture_case.diagnostic_kind {
        StylizedFactDiagnosticKind::ActivityClustering => {
            let payload = serde_json::from_value::<ActivityClusteringRequestPayload>(
                fixture_case.request_payload.clone(),
            )
            .map_err(|source| {
                format!("activity_clustering request_payload parse failed: {source}")
            })?;
            let report = run_activity_clustering_diagnostics(payload.into_request())
                .map_err(|source| format!("activity_clustering diagnostics failed: {source}"))?;
            Ok(activity_metrics_from_report(&report))
        }
        StylizedFactDiagnosticKind::TailSkew => {
            let payload = serde_json::from_value::<TailSkewRequestPayload>(
                fixture_case.request_payload.clone(),
            )
            .map_err(|source| format!("tail_skew request_payload parse failed: {source}"))?;
            let report = run_tail_skew_diagnostics(payload.into_request())
                .map_err(|source| format!("tail_skew diagnostics failed: {source}"))?;
            Ok(tail_skew_metrics_from_report(&report))
        }
        StylizedFactDiagnosticKind::VolatilityPersistence => {
            let payload = serde_json::from_value::<VolatilityPersistenceRequestPayload>(
                fixture_case.request_payload.clone(),
            )
            .map_err(|source| {
                format!("volatility_persistence request_payload parse failed: {source}")
            })?;
            let report = run_volatility_persistence_diagnostics(payload.into_request())
                .map_err(|source| format!("volatility_persistence diagnostics failed: {source}"))?;
            Ok(volatility_metrics_from_report(&report))
        }
        StylizedFactDiagnosticKind::GapBehavior => {
            let payload = serde_json::from_value::<GapBehaviorRequestPayload>(
                fixture_case.request_payload.clone(),
            )
            .map_err(|source| format!("gap_behavior request_payload parse failed: {source}"))?;
            let report = run_gap_behavior_diagnostics(payload.into_request())
                .map_err(|source| format!("gap_behavior diagnostics failed: {source}"))?;
            Ok(gap_behavior_metrics_from_report(&report))
        }
    }
}

fn compare_optional_float(
    mismatches: &mut Vec<String>,
    fixture_id: &str,
    metric_key: &MetricKey,
    field_name: &str,
    expected: Option<f64>,
    observed: Option<f64>,
    tolerance: f64,
    debug_context: &str,
) {
    match (expected, observed) {
        (Some(expected_value), Some(observed_value)) => {
            if !within_tolerance(expected_value, observed_value, tolerance) {
                mismatches.push(format!(
                    "fixture_id={} horizon={} metric_id={} metric_variant={:?} {} mismatch expected={} observed={} tolerance={} {}",
                    fixture_id,
                    metric_key.horizon_label,
                    metric_key.metric_id,
                    metric_key.metric_variant,
                    field_name,
                    expected_value,
                    observed_value,
                    tolerance,
                    debug_context
                ));
            }
        }
        (None, None) => {}
        _ => {
            mismatches.push(format!(
                "fixture_id={} horizon={} metric_id={} metric_variant={:?} {} presence mismatch expected={:?} observed={:?} {}",
                fixture_id,
                metric_key.horizon_label,
                metric_key.metric_id,
                metric_key.metric_variant,
                field_name,
                expected,
                observed,
                debug_context
            ));
        }
    }
}

fn compare_optional_confidence_interval(
    mismatches: &mut Vec<String>,
    fixture_id: &str,
    metric_key: &MetricKey,
    field_name: &str,
    expected: Option<StylizedFactExpectedConfidenceInterval>,
    observed: Option<ConfidenceIntervalSnapshot>,
    tolerance: f64,
    debug_context: &str,
) {
    match (expected, observed) {
        (Some(expected_interval), Some(observed_interval)) => {
            if !within_tolerance(
                expected_interval.lower_bound,
                observed_interval.lower_bound,
                tolerance,
            ) || !within_tolerance(
                expected_interval.upper_bound,
                observed_interval.upper_bound,
                tolerance,
            ) || !within_tolerance(
                expected_interval.confidence_level,
                observed_interval.confidence_level,
                tolerance,
            ) {
                mismatches.push(format!(
                    "fixture_id={} horizon={} metric_id={} metric_variant={:?} {} mismatch expected=({}, {}, {}) observed=({}, {}, {}) tolerance={} {}",
                    fixture_id,
                    metric_key.horizon_label,
                    metric_key.metric_id,
                    metric_key.metric_variant,
                    field_name,
                    expected_interval.lower_bound,
                    expected_interval.upper_bound,
                    expected_interval.confidence_level,
                    observed_interval.lower_bound,
                    observed_interval.upper_bound,
                    observed_interval.confidence_level,
                    tolerance,
                    debug_context
                ));
            }
        }
        (None, None) => {}
        _ => {
            mismatches.push(format!(
                "fixture_id={} horizon={} metric_id={} metric_variant={:?} {} presence mismatch expected={:?} observed={:?} {}",
                fixture_id,
                metric_key.horizon_label,
                metric_key.metric_id,
                metric_key.metric_variant,
                field_name,
                expected,
                observed,
                debug_context
            ));
        }
    }
}

fn within_tolerance(expected: f64, observed: f64, tolerance: f64) -> bool {
    let absolute_difference = (expected - observed).abs();
    if absolute_difference <= tolerance {
        return true;
    }

    let comparison_scale = expected.abs().max(observed.abs()).max(tolerance).max(1.0);
    absolute_difference <= tolerance + f64::EPSILON * comparison_scale * 8.0
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MetricKey {
    horizon_label: String,
    metric_id: String,
    metric_variant: Option<String>,
}

impl MetricKey {
    fn from_expected(expected_metric: &StylizedFactExpectedMetricRegressionEntry) -> Self {
        Self {
            horizon_label: expected_metric.horizon_label.clone(),
            metric_id: expected_metric.metric_id.clone(),
            metric_variant: expected_metric.metric_variant.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ConfidenceIntervalSnapshot {
    lower_bound: f64,
    upper_bound: f64,
    confidence_level: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct ObservedMetric {
    status: StylizedFactExpectedMetricStatus,
    empirical_value: Option<f64>,
    simulated_value: Option<f64>,
    absolute_delta: Option<f64>,
    empirical_confidence_interval: Option<ConfidenceIntervalSnapshot>,
    simulated_confidence_interval: Option<ConfidenceIntervalSnapshot>,
    debug_context: String,
}

fn activity_metrics_from_report(
    report: &janiform_analytics_stylized_facts::ActivityClusteringDiagnosticsReport,
) -> BTreeMap<MetricKey, ObservedMetric> {
    let mut observed_metrics = BTreeMap::new();
    for horizon in &report.horizons {
        for metric in [
            &horizon.event_burst_duration_quantile_ratio,
            &horizon.event_interarrival_expected_shortfall_ratio,
            &horizon.volatility_abs_return_lag1_autocorrelation,
        ] {
            observed_metrics.insert(
                MetricKey {
                    horizon_label: horizon.horizon_label.clone(),
                    metric_id: metric.metric_id.to_string(),
                    metric_variant: None,
                },
                ObservedMetric {
                    status: map_status(metric.status),
                    empirical_value: metric.empirical.value,
                    simulated_value: metric.simulated.value,
                    absolute_delta: metric.absolute_delta,
                    empirical_confidence_interval: metric.empirical.confidence_interval.map(
                        |interval| ConfidenceIntervalSnapshot {
                            lower_bound: interval.lower_bound,
                            upper_bound: interval.upper_bound,
                            confidence_level: interval.confidence_level,
                        },
                    ),
                    simulated_confidence_interval: metric.simulated.confidence_interval.map(
                        |interval| ConfidenceIntervalSnapshot {
                            lower_bound: interval.lower_bound,
                            upper_bound: interval.upper_bound,
                            confidence_level: interval.confidence_level,
                        },
                    ),
                    debug_context: format!(
                        "empirical_coverage={}/{} simulated_coverage={}/{}",
                        metric.empirical.sample_coverage.covered_interval_count,
                        metric.empirical.sample_coverage.horizon_interval_count,
                        metric.simulated.sample_coverage.covered_interval_count,
                        metric.simulated.sample_coverage.horizon_interval_count
                    ),
                },
            );
        }
    }

    observed_metrics
}

fn tail_skew_metrics_from_report(
    report: &janiform_analytics_stylized_facts::TailSkewDiagnosticsReport,
) -> BTreeMap<MetricKey, ObservedMetric> {
    let mut observed_metrics = BTreeMap::new();
    for horizon in &report.horizons {
        for metric in &horizon.metrics {
            observed_metrics.insert(
                MetricKey {
                    horizon_label: horizon.horizon_label.clone(),
                    metric_id: metric.metric_id.to_string(),
                    metric_variant: None,
                },
                ObservedMetric {
                    status: map_tail_status(metric.status),
                    empirical_value: metric.empirical.value,
                    simulated_value: metric.simulated.value,
                    absolute_delta: metric.absolute_delta,
                    empirical_confidence_interval: metric.empirical.confidence_interval.map(
                        |interval| ConfidenceIntervalSnapshot {
                            lower_bound: interval.lower_bound,
                            upper_bound: interval.upper_bound,
                            confidence_level: interval.confidence_level,
                        },
                    ),
                    simulated_confidence_interval: metric.simulated.confidence_interval.map(
                        |interval| ConfidenceIntervalSnapshot {
                            lower_bound: interval.lower_bound,
                            upper_bound: interval.upper_bound,
                            confidence_level: interval.confidence_level,
                        },
                    ),
                    debug_context: format!(
                        "empirical_coverage={}/{} simulated_coverage={}/{}",
                        metric.empirical.sample_coverage.covered_interval_count,
                        metric.empirical.sample_coverage.horizon_interval_count,
                        metric.simulated.sample_coverage.covered_interval_count,
                        metric.simulated.sample_coverage.horizon_interval_count
                    ),
                },
            );
        }
    }

    observed_metrics
}

fn volatility_metrics_from_report(
    report: &janiform_analytics_stylized_facts::VolatilityPersistenceDiagnosticsReport,
) -> BTreeMap<MetricKey, ObservedMetric> {
    let mut observed_metrics = BTreeMap::new();
    for horizon in &report.horizons {
        for lag_comparison in &horizon.lag_comparisons {
            observed_metrics.insert(
                MetricKey {
                    horizon_label: horizon.horizon_label.clone(),
                    metric_id: lag_comparison.metric_id.clone(),
                    metric_variant: Some(lag_comparison.lag_label.clone()),
                },
                ObservedMetric {
                    status: map_volatility_status(lag_comparison.status),
                    empirical_value: lag_comparison.empirical.value,
                    simulated_value: lag_comparison.simulated.value,
                    absolute_delta: lag_comparison.absolute_delta,
                    empirical_confidence_interval: lag_comparison
                        .empirical
                        .confidence_interval
                        .map(|interval| ConfidenceIntervalSnapshot {
                            lower_bound: interval.lower_bound,
                            upper_bound: interval.upper_bound,
                            confidence_level: interval.confidence_level,
                        }),
                    simulated_confidence_interval: lag_comparison
                        .simulated
                        .confidence_interval
                        .map(|interval| ConfidenceIntervalSnapshot {
                            lower_bound: interval.lower_bound,
                            upper_bound: interval.upper_bound,
                            confidence_level: interval.confidence_level,
                        }),
                    debug_context: format!(
                        "empirical_coverage={}/{} simulated_coverage={}/{}",
                        lag_comparison
                            .empirical
                            .sample_coverage
                            .covered_interval_count,
                        lag_comparison
                            .empirical
                            .sample_coverage
                            .horizon_interval_count,
                        lag_comparison
                            .simulated
                            .sample_coverage
                            .covered_interval_count,
                        lag_comparison
                            .simulated
                            .sample_coverage
                            .horizon_interval_count
                    ),
                },
            );
        }
    }

    observed_metrics
}

fn gap_behavior_metrics_from_report(
    report: &janiform_analytics_stylized_facts::GapBehaviorDiagnosticsReport,
) -> BTreeMap<MetricKey, ObservedMetric> {
    let mut observed_metrics = BTreeMap::new();
    for horizon in &report.horizons {
        for metric in [
            &horizon.open_jump_abs_tail_expected_shortfall_ratio,
            &horizon.closed_gap_abs_tail_expected_shortfall_ratio,
            &horizon.closed_gap_vs_open_jump_wasserstein_distance,
        ] {
            observed_metrics.insert(
                MetricKey {
                    horizon_label: horizon.horizon_label.clone(),
                    metric_id: metric.metric_id.clone(),
                    metric_variant: None,
                },
                ObservedMetric {
                    status: map_gap_status(metric.status),
                    empirical_value: metric.empirical.value,
                    simulated_value: metric.simulated.value,
                    absolute_delta: metric.absolute_delta,
                    empirical_confidence_interval: metric.empirical.confidence_interval.map(
                        |interval| ConfidenceIntervalSnapshot {
                            lower_bound: interval.lower_bound,
                            upper_bound: interval.upper_bound,
                            confidence_level: interval.confidence_level,
                        },
                    ),
                    simulated_confidence_interval: metric.simulated.confidence_interval.map(
                        |interval| ConfidenceIntervalSnapshot {
                            lower_bound: interval.lower_bound,
                            upper_bound: interval.upper_bound,
                            confidence_level: interval.confidence_level,
                        },
                    ),
                    debug_context: format!(
                        "empirical_coverage={}/{} simulated_coverage={}/{}",
                        metric.empirical.sample_coverage.covered_interval_count,
                        metric.empirical.sample_coverage.horizon_interval_count,
                        metric.simulated.sample_coverage.covered_interval_count,
                        metric.simulated.sample_coverage.horizon_interval_count
                    ),
                },
            );
        }
    }

    observed_metrics
}

fn map_status(
    status: janiform_analytics_stylized_facts::ActivityClusteringMetricStatus,
) -> StylizedFactExpectedMetricStatus {
    match status {
        janiform_analytics_stylized_facts::ActivityClusteringMetricStatus::Available => {
            StylizedFactExpectedMetricStatus::Available
        }
        janiform_analytics_stylized_facts::ActivityClusteringMetricStatus::InsufficientData => {
            StylizedFactExpectedMetricStatus::InsufficientData
        }
    }
}

fn map_tail_status(
    status: janiform_analytics_stylized_facts::TailSkewMetricStatus,
) -> StylizedFactExpectedMetricStatus {
    match status {
        janiform_analytics_stylized_facts::TailSkewMetricStatus::Available => {
            StylizedFactExpectedMetricStatus::Available
        }
        janiform_analytics_stylized_facts::TailSkewMetricStatus::InsufficientData => {
            StylizedFactExpectedMetricStatus::InsufficientData
        }
    }
}

fn map_volatility_status(
    status: janiform_analytics_stylized_facts::VolatilityPersistenceMetricStatus,
) -> StylizedFactExpectedMetricStatus {
    match status {
        janiform_analytics_stylized_facts::VolatilityPersistenceMetricStatus::Available => {
            StylizedFactExpectedMetricStatus::Available
        }
        janiform_analytics_stylized_facts::VolatilityPersistenceMetricStatus::InsufficientData => {
            StylizedFactExpectedMetricStatus::InsufficientData
        }
    }
}

fn map_gap_status(
    status: janiform_analytics_stylized_facts::GapBehaviorMetricStatus,
) -> StylizedFactExpectedMetricStatus {
    match status {
        janiform_analytics_stylized_facts::GapBehaviorMetricStatus::Available => {
            StylizedFactExpectedMetricStatus::Available
        }
        janiform_analytics_stylized_facts::GapBehaviorMetricStatus::InsufficientData => {
            StylizedFactExpectedMetricStatus::InsufficientData
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityClusteringRequestPayload {
    analysis_start_utc_nanos: i64,
    analysis_end_utc_nanos: i64,
    empirical_dataset: ActivityClusteringDatasetPayload,
    simulated_dataset: ActivityClusteringDatasetPayload,
    horizon_buckets: Vec<HorizonBucketPayload>,
    bootstrap_configuration: BlockBootstrapPayload,
    sample_sufficiency_policy: ActivityClusteringSampleSufficiencyPayload,
    threshold_policy: ActivityClusteringThresholdPolicyPayload,
}

impl ActivityClusteringRequestPayload {
    fn into_request(self) -> ActivityClusteringDiagnosticsRequest {
        ActivityClusteringDiagnosticsRequest {
            analysis_start_utc_nanos: self.analysis_start_utc_nanos,
            analysis_end_utc_nanos: self.analysis_end_utc_nanos,
            empirical_dataset: self.empirical_dataset.into_input(),
            simulated_dataset: self.simulated_dataset.into_input(),
            horizon_buckets: self
                .horizon_buckets
                .into_iter()
                .map(|bucket| ActivityClusteringHorizonBucket {
                    horizon_label: bucket.horizon_label,
                    horizon_duration_nanos: bucket.horizon_duration_nanos,
                })
                .collect(),
            bootstrap_configuration: ActivityClusteringBlockBootstrapConfiguration {
                confidence_level: self.bootstrap_configuration.confidence_level,
                replication_count: self.bootstrap_configuration.replication_count,
                block_length_interval_count: self
                    .bootstrap_configuration
                    .block_length_interval_count,
                random_seed: self.bootstrap_configuration.random_seed,
            },
            sample_sufficiency_policy: self.sample_sufficiency_policy.into_policy(),
            threshold_policy: self.threshold_policy.into_policy(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityClusteringDatasetPayload {
    dataset_id: String,
    event_timestamps_utc_nanos: Vec<i64>,
    return_observations: Vec<ReturnObservationPayload>,
}

impl ActivityClusteringDatasetPayload {
    fn into_input(self) -> ActivityClusteringDatasetInput {
        ActivityClusteringDatasetInput {
            dataset_id: self.dataset_id,
            event_timestamps_utc_nanos: self.event_timestamps_utc_nanos,
            return_observations: self
                .return_observations
                .into_iter()
                .map(|observation| ActivityClusteringReturnObservation {
                    timestamp_utc_nanos: observation.timestamp_utc_nanos,
                    return_increment: observation.return_increment,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityClusteringSampleSufficiencyPayload {
    event_burst_duration_quantile_ratio: MetricSampleSufficiencyPayload,
    event_interarrival_expected_shortfall_ratio: MetricSampleSufficiencyPayload,
    volatility_abs_return_lag1_autocorrelation: MetricSampleSufficiencyPayload,
}

impl ActivityClusteringSampleSufficiencyPayload {
    fn into_policy(self) -> ActivityClusteringSampleSufficiencyPolicy {
        ActivityClusteringSampleSufficiencyPolicy {
            event_burst_duration_quantile_ratio: self
                .event_burst_duration_quantile_ratio
                .into_activity_policy(),
            event_interarrival_expected_shortfall_ratio: self
                .event_interarrival_expected_shortfall_ratio
                .into_activity_policy(),
            volatility_abs_return_lag1_autocorrelation: self
                .volatility_abs_return_lag1_autocorrelation
                .into_activity_policy(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityClusteringThresholdPolicyPayload {
    comparison_policy_id: String,
    event_burst_duration_quantile_ratio_max_absolute_delta: f64,
    event_interarrival_expected_shortfall_ratio_max_absolute_delta: f64,
    volatility_abs_return_lag1_autocorrelation_max_absolute_delta: f64,
}

impl ActivityClusteringThresholdPolicyPayload {
    fn into_policy(self) -> ActivityClusteringThresholdPolicy {
        ActivityClusteringThresholdPolicy {
            comparison_policy_id: self.comparison_policy_id,
            event_burst_duration_quantile_ratio_max_absolute_delta: self
                .event_burst_duration_quantile_ratio_max_absolute_delta,
            event_interarrival_expected_shortfall_ratio_max_absolute_delta: self
                .event_interarrival_expected_shortfall_ratio_max_absolute_delta,
            volatility_abs_return_lag1_autocorrelation_max_absolute_delta: self
                .volatility_abs_return_lag1_autocorrelation_max_absolute_delta,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TailSkewRequestPayload {
    analysis_start_utc_nanos: i64,
    analysis_end_utc_nanos: i64,
    empirical_dataset: TailSkewDatasetPayload,
    simulated_dataset: TailSkewDatasetPayload,
    horizon_buckets: Vec<HorizonBucketPayload>,
    bootstrap_configuration: BlockBootstrapPayload,
    sample_sufficiency_policy: TailSkewSampleSufficiencyPayload,
}

impl TailSkewRequestPayload {
    fn into_request(self) -> TailSkewDiagnosticsRequest {
        TailSkewDiagnosticsRequest {
            analysis_start_utc_nanos: self.analysis_start_utc_nanos,
            analysis_end_utc_nanos: self.analysis_end_utc_nanos,
            empirical_dataset: self.empirical_dataset.into_input(),
            simulated_dataset: self.simulated_dataset.into_input(),
            horizon_buckets: self
                .horizon_buckets
                .into_iter()
                .map(|bucket| TailSkewHorizonBucket {
                    horizon_label: bucket.horizon_label,
                    horizon_duration_nanos: bucket.horizon_duration_nanos,
                })
                .collect(),
            bootstrap_configuration: TailSkewBlockBootstrapConfiguration {
                confidence_level: self.bootstrap_configuration.confidence_level,
                replication_count: self.bootstrap_configuration.replication_count,
                block_length_interval_count: self
                    .bootstrap_configuration
                    .block_length_interval_count,
                random_seed: self.bootstrap_configuration.random_seed,
            },
            sample_sufficiency_policy: self.sample_sufficiency_policy.into_policy(),
            cpu_count: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TailSkewDatasetPayload {
    dataset_id: String,
    return_observations: Vec<ReturnObservationPayload>,
}

impl TailSkewDatasetPayload {
    fn into_input(self) -> TailSkewDatasetInput {
        TailSkewDatasetInput {
            dataset_id: self.dataset_id,
            return_observations: self
                .return_observations
                .into_iter()
                .map(|observation| TailSkewReturnObservation {
                    timestamp_utc_nanos: observation.timestamp_utc_nanos,
                    return_increment: observation.return_increment,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TailSkewSampleSufficiencyPayload {
    returns_abs_quantile_shape_ratio_q99_q95: MetricSampleSufficiencyPayload,
    returns_left_tail_expected_shortfall_to_var_ratio: MetricSampleSufficiencyPayload,
    returns_bowley_skewness_quantile: MetricSampleSufficiencyPayload,
    returns_signed_tail_expected_shortfall_imbalance: MetricSampleSufficiencyPayload,
    returns_l_moment_lambda2: MetricSampleSufficiencyPayload,
    returns_l_moment_tau3: MetricSampleSufficiencyPayload,
    returns_central_variance: MetricSampleSufficiencyPayload,
    returns_central_skewness: MetricSampleSufficiencyPayload,
    returns_central_excess_kurtosis: MetricSampleSufficiencyPayload,
}

impl TailSkewSampleSufficiencyPayload {
    fn into_policy(self) -> TailSkewSampleSufficiencyPolicy {
        TailSkewSampleSufficiencyPolicy {
            returns_abs_quantile_shape_ratio_q99_q95: self
                .returns_abs_quantile_shape_ratio_q99_q95
                .into_tail_policy(),
            returns_left_tail_expected_shortfall_to_var_ratio: self
                .returns_left_tail_expected_shortfall_to_var_ratio
                .into_tail_policy(),
            returns_bowley_skewness_quantile: self
                .returns_bowley_skewness_quantile
                .into_tail_policy(),
            returns_signed_tail_expected_shortfall_imbalance: self
                .returns_signed_tail_expected_shortfall_imbalance
                .into_tail_policy(),
            returns_l_moment_lambda2: self.returns_l_moment_lambda2.into_tail_policy(),
            returns_l_moment_tau3: self.returns_l_moment_tau3.into_tail_policy(),
            returns_central_variance: self.returns_central_variance.into_tail_policy(),
            returns_central_skewness: self.returns_central_skewness.into_tail_policy(),
            returns_central_excess_kurtosis: self
                .returns_central_excess_kurtosis
                .into_tail_policy(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct VolatilityPersistenceRequestPayload {
    analysis_start_utc_nanos: i64,
    analysis_end_utc_nanos: i64,
    empirical_dataset: VolatilityPersistenceDatasetPayload,
    simulated_dataset: VolatilityPersistenceDatasetPayload,
    horizon_buckets: Vec<HorizonBucketPayload>,
    lag_settings: Vec<LagSettingPayload>,
    bootstrap_configuration: BlockBootstrapPayload,
    sample_sufficiency_policy: VolatilityPersistenceSampleSufficiencyPayload,
    threshold_policy: VolatilityPersistenceThresholdPolicyPayload,
}

impl VolatilityPersistenceRequestPayload {
    fn into_request(self) -> VolatilityPersistenceDiagnosticsRequest {
        VolatilityPersistenceDiagnosticsRequest {
            analysis_start_utc_nanos: self.analysis_start_utc_nanos,
            analysis_end_utc_nanos: self.analysis_end_utc_nanos,
            empirical_dataset: self.empirical_dataset.into_input(),
            simulated_dataset: self.simulated_dataset.into_input(),
            horizon_buckets: self
                .horizon_buckets
                .into_iter()
                .map(|bucket| VolatilityPersistenceHorizonBucket {
                    horizon_label: bucket.horizon_label,
                    horizon_duration_nanos: bucket.horizon_duration_nanos,
                })
                .collect(),
            lag_settings: self
                .lag_settings
                .into_iter()
                .map(|lag_setting| VolatilityPersistenceLagSetting {
                    lag_label: lag_setting.lag_label,
                    lag_interval_count: lag_setting.lag_interval_count,
                })
                .collect(),
            bootstrap_configuration: VolatilityPersistenceBlockBootstrapConfiguration {
                confidence_level: self.bootstrap_configuration.confidence_level,
                replication_count: self.bootstrap_configuration.replication_count,
                block_length_interval_count: self
                    .bootstrap_configuration
                    .block_length_interval_count,
                random_seed: self.bootstrap_configuration.random_seed,
            },
            sample_sufficiency_policy: self.sample_sufficiency_policy.into_policy(),
            threshold_policy: self.threshold_policy.into_policy(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct VolatilityPersistenceDatasetPayload {
    dataset_id: String,
    return_observations: Vec<ReturnObservationPayload>,
}

impl VolatilityPersistenceDatasetPayload {
    fn into_input(self) -> VolatilityPersistenceDatasetInput {
        VolatilityPersistenceDatasetInput {
            dataset_id: self.dataset_id,
            return_observations: self
                .return_observations
                .into_iter()
                .map(|observation| VolatilityPersistenceReturnObservation {
                    timestamp_utc_nanos: observation.timestamp_utc_nanos,
                    return_increment: observation.return_increment,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct VolatilityPersistenceSampleSufficiencyPayload {
    lagged_abs_return_autocorrelation: MetricSampleSufficiencyPayload,
}

impl VolatilityPersistenceSampleSufficiencyPayload {
    fn into_policy(self) -> VolatilityPersistenceSampleSufficiencyPolicy {
        VolatilityPersistenceSampleSufficiencyPolicy {
            lagged_abs_return_autocorrelation: self
                .lagged_abs_return_autocorrelation
                .into_volatility_policy(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct VolatilityPersistenceThresholdPolicyPayload {
    comparison_policy_id: String,
    lagged_abs_return_autocorrelation_max_absolute_delta: f64,
}

impl VolatilityPersistenceThresholdPolicyPayload {
    fn into_policy(self) -> VolatilityPersistenceThresholdPolicy {
        VolatilityPersistenceThresholdPolicy {
            comparison_policy_id: self.comparison_policy_id,
            lagged_abs_return_autocorrelation_max_absolute_delta: self
                .lagged_abs_return_autocorrelation_max_absolute_delta,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GapBehaviorRequestPayload {
    analysis_start_utc_nanos: i64,
    analysis_end_utc_nanos: i64,
    empirical_dataset: GapBehaviorDatasetPayload,
    simulated_dataset: GapBehaviorDatasetPayload,
    horizon_buckets: Vec<HorizonBucketPayload>,
    bootstrap_configuration: BlockBootstrapPayload,
    sample_sufficiency_policy: GapBehaviorSampleSufficiencyPayload,
    threshold_policy: GapBehaviorThresholdPolicyPayload,
}

impl GapBehaviorRequestPayload {
    fn into_request(self) -> GapBehaviorDiagnosticsRequest {
        GapBehaviorDiagnosticsRequest {
            analysis_start_utc_nanos: self.analysis_start_utc_nanos,
            analysis_end_utc_nanos: self.analysis_end_utc_nanos,
            empirical_dataset: self.empirical_dataset.into_input(),
            simulated_dataset: self.simulated_dataset.into_input(),
            horizon_buckets: self
                .horizon_buckets
                .into_iter()
                .map(|bucket| GapBehaviorHorizonBucket {
                    horizon_label: bucket.horizon_label,
                    horizon_duration_nanos: bucket.horizon_duration_nanos,
                })
                .collect(),
            bootstrap_configuration: GapBehaviorBlockBootstrapConfiguration {
                confidence_level: self.bootstrap_configuration.confidence_level,
                replication_count: self.bootstrap_configuration.replication_count,
                block_length_interval_count: self
                    .bootstrap_configuration
                    .block_length_interval_count,
                random_seed: self.bootstrap_configuration.random_seed,
            },
            sample_sufficiency_policy: self.sample_sufficiency_policy.into_policy(),
            threshold_policy: self.threshold_policy.into_policy(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GapBehaviorDatasetPayload {
    dataset_id: String,
    scenarios: Vec<GapBehaviorScenarioPayload>,
}

impl GapBehaviorDatasetPayload {
    fn into_input(self) -> GapBehaviorDatasetInput {
        GapBehaviorDatasetInput {
            dataset_id: self.dataset_id,
            scenarios: self
                .scenarios
                .into_iter()
                .map(|scenario| scenario.into_input())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GapBehaviorScenarioPayload {
    scenario_id: String,
    exchange_code: String,
    scenario_class: String,
    deterministic_replay_metadata: GapDeterministicReplayMetadataPayload,
    issue_traceability: GapIssueTraceabilityPayload,
    tags: Vec<String>,
    event_observations: Vec<GapEventObservationPayload>,
}

impl GapBehaviorScenarioPayload {
    fn into_input(self) -> GapBehaviorScenarioInput {
        GapBehaviorScenarioInput {
            scenario_id: self.scenario_id,
            exchange_code: self.exchange_code,
            scenario_class: parse_scenario_class(self.scenario_class.as_str()),
            deterministic_replay_metadata: self.deterministic_replay_metadata.into_metadata(),
            issue_traceability: self.issue_traceability.into_metadata(),
            tags: self.tags,
            event_observations: self
                .event_observations
                .into_iter()
                .map(|event| event.into_input())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GapBehaviorSampleSufficiencyPayload {
    open_jump_abs_tail_expected_shortfall_ratio: MetricSampleSufficiencyPayload,
    closed_gap_abs_tail_expected_shortfall_ratio: MetricSampleSufficiencyPayload,
    closed_gap_vs_open_jump_wasserstein_distance: MetricSampleSufficiencyPayload,
}

impl GapBehaviorSampleSufficiencyPayload {
    fn into_policy(self) -> GapBehaviorSampleSufficiencyPolicy {
        GapBehaviorSampleSufficiencyPolicy {
            open_jump_abs_tail_expected_shortfall_ratio: self
                .open_jump_abs_tail_expected_shortfall_ratio
                .into_gap_policy(),
            closed_gap_abs_tail_expected_shortfall_ratio: self
                .closed_gap_abs_tail_expected_shortfall_ratio
                .into_gap_policy(),
            closed_gap_vs_open_jump_wasserstein_distance: self
                .closed_gap_vs_open_jump_wasserstein_distance
                .into_gap_policy(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GapBehaviorThresholdPolicyPayload {
    comparison_policy_id: String,
    open_jump_abs_tail_expected_shortfall_ratio_max_absolute_delta: f64,
    closed_gap_abs_tail_expected_shortfall_ratio_max_absolute_delta: f64,
    closed_gap_vs_open_jump_wasserstein_distance_max_absolute_delta: f64,
}

impl GapBehaviorThresholdPolicyPayload {
    fn into_policy(self) -> GapBehaviorThresholdPolicy {
        GapBehaviorThresholdPolicy {
            comparison_policy_id: self.comparison_policy_id,
            open_jump_abs_tail_expected_shortfall_ratio_max_absolute_delta: self
                .open_jump_abs_tail_expected_shortfall_ratio_max_absolute_delta,
            closed_gap_abs_tail_expected_shortfall_ratio_max_absolute_delta: self
                .closed_gap_abs_tail_expected_shortfall_ratio_max_absolute_delta,
            closed_gap_vs_open_jump_wasserstein_distance_max_absolute_delta: self
                .closed_gap_vs_open_jump_wasserstein_distance_max_absolute_delta,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct HorizonBucketPayload {
    horizon_label: String,
    horizon_duration_nanos: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LagSettingPayload {
    lag_label: String,
    lag_interval_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockBootstrapPayload {
    confidence_level: f64,
    replication_count: usize,
    block_length_interval_count: usize,
    random_seed: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricSampleSufficiencyPayload {
    minimum_sample_count: usize,
    minimum_effective_sample_count: usize,
    minimum_tail_observation_count: usize,
    minimum_horizon_coverage_ratio: f64,
}

impl MetricSampleSufficiencyPayload {
    fn into_activity_policy(self) -> ActivityClusteringMetricSampleSufficiencyPolicy {
        ActivityClusteringMetricSampleSufficiencyPolicy {
            minimum_sample_count: self.minimum_sample_count,
            minimum_effective_sample_count: self.minimum_effective_sample_count,
            minimum_tail_observation_count: self.minimum_tail_observation_count,
            minimum_horizon_coverage_ratio: self.minimum_horizon_coverage_ratio,
        }
    }

    fn into_tail_policy(self) -> TailSkewMetricSampleSufficiencyPolicy {
        TailSkewMetricSampleSufficiencyPolicy {
            minimum_sample_count: self.minimum_sample_count,
            minimum_effective_sample_count: self.minimum_effective_sample_count,
            minimum_tail_observation_count: self.minimum_tail_observation_count,
            minimum_horizon_coverage_ratio: self.minimum_horizon_coverage_ratio,
        }
    }

    fn into_volatility_policy(self) -> VolatilityPersistenceMetricSampleSufficiencyPolicy {
        VolatilityPersistenceMetricSampleSufficiencyPolicy {
            minimum_sample_count: self.minimum_sample_count,
            minimum_effective_sample_count: self.minimum_effective_sample_count,
            minimum_tail_observation_count: self.minimum_tail_observation_count,
            minimum_horizon_coverage_ratio: self.minimum_horizon_coverage_ratio,
        }
    }

    fn into_gap_policy(self) -> GapBehaviorMetricSampleSufficiencyPolicy {
        GapBehaviorMetricSampleSufficiencyPolicy {
            minimum_sample_count: self.minimum_sample_count,
            minimum_effective_sample_count: self.minimum_effective_sample_count,
            minimum_tail_observation_count: self.minimum_tail_observation_count,
            minimum_horizon_coverage_ratio: self.minimum_horizon_coverage_ratio,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReturnObservationPayload {
    timestamp_utc_nanos: i64,
    return_increment: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GapDeterministicReplayMetadataPayload {
    replay_baseline_id: String,
    event_calendar_binding_id: String,
    jump_observation_binding_id: String,
    gap_observation_binding_id: String,
    candidate_selection_policy_id: String,
    candidate_selection_rank: u64,
}

impl GapDeterministicReplayMetadataPayload {
    fn into_metadata(self) -> OpenClosedGapDeterministicReplayMetadata {
        OpenClosedGapDeterministicReplayMetadata {
            replay_baseline_id: self.replay_baseline_id,
            event_calendar_binding_id: self.event_calendar_binding_id,
            jump_observation_binding_id: self.jump_observation_binding_id,
            gap_observation_binding_id: self.gap_observation_binding_id,
            candidate_selection_policy_id: self.candidate_selection_policy_id,
            candidate_selection_rank: self.candidate_selection_rank,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GapIssueTraceabilityPayload {
    issue_id: String,
    semantic_focus_tags: Vec<String>,
}

impl GapIssueTraceabilityPayload {
    fn into_metadata(self) -> OpenClosedGapIssueTraceabilityMetadata {
        OpenClosedGapIssueTraceabilityMetadata {
            issue_id: self.issue_id,
            semantic_focus_tags: self.semantic_focus_tags,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GapEventObservationPayload {
    event_id: String,
    timestamp_utc_nanos: i64,
    market_state: String,
    closed_window_regime: Option<String>,
    activity_coupling_increment: f64,
    open_jump_increment: f64,
    closed_gap_increment: f64,
}

impl GapEventObservationPayload {
    fn into_input(self) -> GapBehaviorEventObservation {
        GapBehaviorEventObservation {
            event_id: self.event_id,
            timestamp_utc_nanos: self.timestamp_utc_nanos,
            market_state: parse_market_state(self.market_state.as_str()),
            closed_window_regime: self
                .closed_window_regime
                .as_deref()
                .map(parse_closed_window_regime),
            activity_coupling_increment: self.activity_coupling_increment,
            open_jump_increment: self.open_jump_increment,
            closed_gap_increment: self.closed_gap_increment,
        }
    }
}

fn parse_scenario_class(raw_value: &str) -> NewsSemanticsScenarioClass {
    match raw_value {
        "known_only" => NewsSemanticsScenarioClass::KnownOnly,
        "surprise_only" => NewsSemanticsScenarioClass::SurpriseOnly,
        "mixed_news" => NewsSemanticsScenarioClass::MixedNews,
        unsupported => panic!("unsupported scenario_class in fixture payload: {unsupported}"),
    }
}

fn parse_market_state(raw_value: &str) -> GapBehaviorMarketState {
    match raw_value {
        "open" => GapBehaviorMarketState::Open,
        "closed" => GapBehaviorMarketState::Closed,
        unsupported => panic!("unsupported market_state in fixture payload: {unsupported}"),
    }
}

fn parse_closed_window_regime(raw_value: &str) -> GapBehaviorClosedWindowRegime {
    match raw_value {
        "overnight" => GapBehaviorClosedWindowRegime::Overnight,
        "weekend" => GapBehaviorClosedWindowRegime::Weekend,
        "other_closed" => GapBehaviorClosedWindowRegime::OtherClosed,
        unsupported => panic!("unsupported closed_window_regime in fixture payload: {unsupported}"),
    }
}
