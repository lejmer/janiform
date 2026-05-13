mod support;

use std::collections::BTreeSet;
use std::fs;

use chrono::{DateTime, Utc};
use janiform_engine_simulation::{
    MeanReversionBoundaryState, MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
    SeededTickEventRecord, SeededTickEventSide, SeededTickSimulationModelInputs,
    SeededTickSimulationRunConfig, run_seeded_tick_event_simulation,
};
use janiform_foundations::time::NANOS_PER_HOUR;
use janiform_market_time::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader};
use janiform_market_time::hourly_factors::{
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD,
    HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION, HOURLY_FACTOR_FALL_BACK_POLICY,
    HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY, HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
    HOURLY_FACTOR_SPRING_FORWARD_POLICY, HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
    HourlyFactorContractDiagnostics, HourlyFactorContractOutput,
    HourlyFactorMaterializationMetadata,
};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use serde::Deserialize;
use support::hawkes_failure_diagnostics::{
    HawkesFailureDiagnosticCategory, HawkesFailureDiagnosticReport,
    HawkesFailureDiagnosticViolation, build_hawkes_failure_diagnostic_report,
    classify_statistical_violation_category, render_hawkes_failure_diagnostic_report,
};
use support::hawkes_invariant_catalog::{
    HawkesInvariantCheckClass, assert_hawkes_invariant_catalog_ids_have_check_class,
};
use support::validation_fixture::fixtures_root_path;

const CLUSTER_SIGNATURE_REFERENCE_BANDS_SCHEMA_VERSION: &str =
    "hawkes.cluster_signature.reference_bands.v1";
const CLUSTER_SIGNATURE_REFERENCE_BANDS_SET_ID: &str =
    "hawkes_cluster_signature_regression_bands_v1";

const START_TIME_UTC_NANOS: i64 = 1_704_067_200_000_000_000;
#[derive(Debug, Clone, Copy, PartialEq)]
struct ClusteringMetrics {
    mean_fano_factor: f64,
    mean_lag_one_autocorrelation: f64,
    non_grid_run_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HorizonSummary {
    mean_total_event_count: f64,
    mean_buy_share: f64,
    mean_sell_share: f64,
    mean_log_sell_to_buy_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScaleConsistencyMetrics {
    aggregated_summary: HorizonSummary,
    direct_summary: HorizonSummary,
    relative_total_event_count_delta: f64,
    absolute_buy_share_delta: f64,
    absolute_sell_share_delta: f64,
    absolute_log_sell_to_buy_ratio_delta: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricBand {
    lower_bound: f64,
    upper_bound: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClusteringReferenceBands {
    seed_start: u64,
    seed_count: usize,
    interval_count: usize,
    interval_duration_nanos: i64,
    seasonality_factor: f64,
    news_intensity: f64,
    reference_bands: ClusteringMetricBands,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClusteringMetricBands {
    fano_factor_gap: MetricBand,
    lag_one_autocorrelation_gap: MetricBand,
    clustered_lag_one_autocorrelation: MetricBand,
    clustered_non_grid_run_ratio: MetricBand,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaleConsistencyReferenceBands {
    seed_start: u64,
    seed_count: usize,
    total_duration_nanos: i64,
    fine_interval_duration_nanos: i64,
    seasonality_factor: f64,
    news_intensity: f64,
    horizon_bands: Vec<ScaleConsistencyHorizonReferenceBands>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaleConsistencyHorizonReferenceBands {
    horizon_label: String,
    coarse_interval_duration_nanos: i64,
    relative_total_event_count_delta: MetricBand,
    absolute_buy_share_delta: MetricBand,
    absolute_sell_share_delta: MetricBand,
    absolute_log_sell_to_buy_ratio_delta: MetricBand,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceBandChangeLogEntry {
    policy_version: String,
    recorded_at_utc: String,
    rationale: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClusterSignatureReferenceBandDocument {
    schema_version: String,
    band_set_id: String,
    band_set_version: String,
    determinism_policy: String,
    clustering_regression: ClusteringReferenceBands,
    scale_consistency_regression: ScaleConsistencyReferenceBands,
    change_log: Vec<ReferenceBandChangeLogEntry>,
}

#[derive(Debug, Clone, PartialEq)]
struct BandViolation {
    panel_id: &'static str,
    horizon_label: Option<String>,
    metric_id: &'static str,
    observed_value: f64,
    lower_bound: f64,
    upper_bound: f64,
    delta_relative_to_band: f64,
}

#[test]
fn cluster_signature_metric_calculation_distinguishes_synthetic_clustered_and_non_clustered_fixtures()
 {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-SAMP-001"],
        HawkesInvariantCheckClass::StatisticalProperty,
    );

    let synthetic_non_clustered_runs = vec![
        vec![3, 4, 3, 4, 3, 4],
        vec![2, 3, 2, 3, 2, 3],
        vec![4, 3, 4, 3, 4, 3],
    ];
    let synthetic_clustered_runs = vec![
        vec![0, 0, 12, 0, 0, 10],
        vec![0, 0, 11, 0, 0, 9],
        vec![0, 0, 10, 0, 0, 12],
    ];

    let non_clustered_metrics = compute_clustering_metrics_from_interval_count_runs(
        &synthetic_non_clustered_runs,
        &[true, true, true],
    );
    let clustered_metrics = compute_clustering_metrics_from_interval_count_runs(
        &synthetic_clustered_runs,
        &[true, true, true],
    );

    assert!(clustered_metrics.mean_fano_factor > non_clustered_metrics.mean_fano_factor);
    assert!(
        clustered_metrics.mean_lag_one_autocorrelation
            > non_clustered_metrics.mean_lag_one_autocorrelation
    );
    assert_eq!(clustered_metrics.non_grid_run_ratio, 1.0);
}

#[test]
fn cluster_signature_batch_metrics_are_reproducible_with_fixed_seed_batches() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-SAMP-001"],
        HawkesInvariantCheckClass::StatisticalProperty,
    );

    let reference_bands_document = load_cluster_signature_reference_band_document();
    let reference_bands = &reference_bands_document.clustering_regression;
    let seed_values = seed_values(reference_bands.seed_start, reference_bands.seed_count);

    let first_baseline_metrics = compute_batch_clustering_metrics(
        &seed_values,
        symmetric_kernel_parameters(),
        reference_bands.interval_count,
        reference_bands.interval_duration_nanos,
        reference_bands.seasonality_factor,
        reference_bands.news_intensity,
    );
    let second_baseline_metrics = compute_batch_clustering_metrics(
        &seed_values,
        symmetric_kernel_parameters(),
        reference_bands.interval_count,
        reference_bands.interval_duration_nanos,
        reference_bands.seasonality_factor,
        reference_bands.news_intensity,
    );
    let first_clustered_metrics = compute_batch_clustering_metrics(
        &seed_values,
        clustered_kernel_parameters(),
        reference_bands.interval_count,
        reference_bands.interval_duration_nanos,
        reference_bands.seasonality_factor,
        reference_bands.news_intensity,
    );
    let second_clustered_metrics = compute_batch_clustering_metrics(
        &seed_values,
        clustered_kernel_parameters(),
        reference_bands.interval_count,
        reference_bands.interval_duration_nanos,
        reference_bands.seasonality_factor,
        reference_bands.news_intensity,
    );

    assert_eq!(first_baseline_metrics, second_baseline_metrics);
    assert_eq!(first_clustered_metrics, second_clustered_metrics);
}

#[test]
fn cluster_signature_regression_reference_bands_pass_for_seeded_batches() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-SAMP-001"],
        HawkesInvariantCheckClass::StatisticalProperty,
    );

    let reference_bands_document = load_cluster_signature_reference_band_document();
    let reference_bands = &reference_bands_document.clustering_regression;
    let seed_values = seed_values(reference_bands.seed_start, reference_bands.seed_count);

    let baseline_metrics = compute_batch_clustering_metrics(
        &seed_values,
        symmetric_kernel_parameters(),
        reference_bands.interval_count,
        reference_bands.interval_duration_nanos,
        reference_bands.seasonality_factor,
        reference_bands.news_intensity,
    );
    let clustered_metrics = compute_batch_clustering_metrics(
        &seed_values,
        clustered_kernel_parameters(),
        reference_bands.interval_count,
        reference_bands.interval_duration_nanos,
        reference_bands.seasonality_factor,
        reference_bands.news_intensity,
    );

    let band_violations = evaluate_clustering_reference_bands(
        &reference_bands.reference_bands,
        baseline_metrics,
        clustered_metrics,
    );

    assert_no_band_violations(
        &band_violations,
        "clustering panel",
        "cluster_signature_regression_reference_bands_pass_for_seeded_batches",
        &["HK-SAMP-001"],
    );
}

#[test]
fn cluster_signature_regression_reports_metric_deltas_when_bands_are_violated() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-SAMP-001"],
        HawkesInvariantCheckClass::StatisticalProperty,
    );

    let reference_bands_document = load_cluster_signature_reference_band_document();
    let reference_bands = &reference_bands_document
        .clustering_regression
        .reference_bands;

    let baseline_metrics = ClusteringMetrics {
        mean_fano_factor: 0.9,
        mean_lag_one_autocorrelation: 0.2,
        non_grid_run_ratio: 0.9,
    };
    let clustered_metrics = ClusteringMetrics {
        mean_fano_factor: 1.0,
        mean_lag_one_autocorrelation: 0.3,
        non_grid_run_ratio: 0.8,
    };

    let band_violations =
        evaluate_clustering_reference_bands(reference_bands, baseline_metrics, clustered_metrics);

    assert!(!band_violations.is_empty());
    assert!(
        band_violations
            .iter()
            .any(|band_violation| band_violation.metric_id == "fano_factor_gap")
    );
    assert!(
        band_violations
            .iter()
            .any(|band_violation| band_violation.metric_id == "clustered_non_grid_run_ratio")
    );
    assert!(
        band_violations
            .iter()
            .all(|band_violation| band_violation.delta_relative_to_band != 0.0)
    );

    let diagnostic_payload = build_band_violation_diagnostic_payload(
        "cluster_signature_regression_reports_metric_deltas_when_bands_are_violated",
        &["HK-SAMP-001"],
        &band_violations,
    );
    let parsed_diagnostic_report: HawkesFailureDiagnosticReport =
        serde_json::from_str(&diagnostic_payload)
            .expect("clustering mismatch diagnostics should parse as valid JSON");

    assert_eq!(
        parsed_diagnostic_report.suite_id,
        "hawkes_cluster_signature_statistical_regression"
    );
    assert_eq!(
        parsed_diagnostic_report.test_name,
        "cluster_signature_regression_reports_metric_deltas_when_bands_are_violated"
    );
    assert_eq!(
        parsed_diagnostic_report.check_class,
        HawkesInvariantCheckClass::StatisticalProperty
    );
    assert_eq!(
        parsed_diagnostic_report.invariant_ids,
        vec!["HK-SAMP-001".to_string()]
    );
    assert_eq!(
        parsed_diagnostic_report.summary.violation_count,
        band_violations.len()
    );
    assert!(
        parsed_diagnostic_report
            .summary
            .category_counts
            .iter()
            .any(|category_count| {
                category_count.category == HawkesFailureDiagnosticCategory::StatisticalDrift
            })
    );
    assert!(
        parsed_diagnostic_report
            .summary
            .category_counts
            .iter()
            .any(|category_count| {
                category_count.category == HawkesFailureDiagnosticCategory::EventTimeGridLeakage
            })
    );
}

#[test]
fn cluster_signature_scale_consistency_reference_bands_pass_for_seeded_horizon_panel() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-SCALE-001"],
        HawkesInvariantCheckClass::StatisticalProperty,
    );

    let reference_bands_document = load_cluster_signature_reference_band_document();
    let reference_bands = &reference_bands_document.scale_consistency_regression;
    let seed_values = seed_values(reference_bands.seed_start, reference_bands.seed_count);

    let mut all_band_violations: Vec<BandViolation> = Vec::new();

    for horizon_reference_bands in &reference_bands.horizon_bands {
        let (aggregated_summaries, direct_summaries) = collect_scale_consistency_summaries(
            &seed_values,
            symmetric_kernel_parameters(),
            reference_bands.total_duration_nanos,
            reference_bands.fine_interval_duration_nanos,
            horizon_reference_bands.coarse_interval_duration_nanos,
            reference_bands.seasonality_factor,
            reference_bands.news_intensity,
        );
        let scale_consistency_metrics =
            compute_scale_consistency_metrics(&aggregated_summaries, &direct_summaries);

        let horizon_violations = evaluate_scale_consistency_reference_bands(
            horizon_reference_bands,
            scale_consistency_metrics,
        );
        all_band_violations.extend(horizon_violations);
    }

    assert_no_band_violations(
        &all_band_violations,
        "scale-consistency panel",
        "cluster_signature_scale_consistency_reference_bands_pass_for_seeded_horizon_panel",
        &["HK-SCALE-001"],
    );
}

#[test]
fn cluster_signature_scale_consistency_reports_metric_deltas_when_bands_are_violated() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-SCALE-001"],
        HawkesInvariantCheckClass::StatisticalProperty,
    );

    let reference_bands_document = load_cluster_signature_reference_band_document();
    let failing_horizon_reference_bands = reference_bands_document
        .scale_consistency_regression
        .horizon_bands
        .iter()
        .find(|horizon_reference_bands| horizon_reference_bands.horizon_label == "1h")
        .expect("reference band set should include 1h horizon");

    let intentionally_failing_metrics = ScaleConsistencyMetrics {
        aggregated_summary: HorizonSummary {
            mean_total_event_count: 0.0,
            mean_buy_share: 0.0,
            mean_sell_share: 0.0,
            mean_log_sell_to_buy_ratio: 0.0,
        },
        direct_summary: HorizonSummary {
            mean_total_event_count: 0.0,
            mean_buy_share: 0.0,
            mean_sell_share: 0.0,
            mean_log_sell_to_buy_ratio: 0.0,
        },
        relative_total_event_count_delta: failing_horizon_reference_bands
            .relative_total_event_count_delta
            .upper_bound
            + 0.05,
        absolute_buy_share_delta: failing_horizon_reference_bands
            .absolute_buy_share_delta
            .upper_bound
            + 0.02,
        absolute_sell_share_delta: failing_horizon_reference_bands
            .absolute_sell_share_delta
            .upper_bound
            + 0.02,
        absolute_log_sell_to_buy_ratio_delta: failing_horizon_reference_bands
            .absolute_log_sell_to_buy_ratio_delta
            .upper_bound
            + 0.10,
    };

    let band_violations = evaluate_scale_consistency_reference_bands(
        failing_horizon_reference_bands,
        intentionally_failing_metrics,
    );

    assert!(!band_violations.is_empty());
    assert!(
        band_violations
            .iter()
            .all(|band_violation| band_violation.horizon_label.as_deref() == Some("1h"))
    );
    assert!(
        band_violations
            .iter()
            .all(|band_violation| band_violation.delta_relative_to_band > 0.0)
    );

    let diagnostic_payload = build_band_violation_diagnostic_payload(
        "cluster_signature_scale_consistency_reports_metric_deltas_when_bands_are_violated",
        &["HK-SCALE-001"],
        &band_violations,
    );
    let parsed_diagnostic_report: HawkesFailureDiagnosticReport =
        serde_json::from_str(&diagnostic_payload)
            .expect("scale-consistency mismatch diagnostics should parse as valid JSON");

    assert_eq!(
        parsed_diagnostic_report.suite_id,
        "hawkes_cluster_signature_statistical_regression"
    );
    assert_eq!(
        parsed_diagnostic_report.test_name,
        "cluster_signature_scale_consistency_reports_metric_deltas_when_bands_are_violated"
    );
    assert_eq!(
        parsed_diagnostic_report.invariant_ids,
        vec!["HK-SCALE-001".to_string()]
    );
    assert!(
        parsed_diagnostic_report
            .summary
            .category_counts
            .iter()
            .any(|category_count| {
                category_count.category == HawkesFailureDiagnosticCategory::ScaleConsistencyDrift
            })
    );
    assert!(
        parsed_diagnostic_report
            .violations
            .iter()
            .all(|violation| violation.scope.contains("horizon=1h"))
    );
}

fn load_cluster_signature_reference_band_document() -> ClusterSignatureReferenceBandDocument {
    let fixture_path = fixtures_root_path()
        .join("hawkes")
        .join("cluster_signature_reference_bands.json");
    let fixture_payload = fs::read_to_string(&fixture_path).unwrap_or_else(|source| {
        panic!(
            "failed to read cluster-signature reference-band fixture `{}`: {source}",
            fixture_path.display()
        )
    });
    let fixture_document =
        serde_json::from_str::<ClusterSignatureReferenceBandDocument>(&fixture_payload)
            .unwrap_or_else(|source| {
                panic!(
                    "failed to parse cluster-signature reference-band fixture `{}`: {source}",
                    fixture_path.display()
                )
            });

    validate_cluster_signature_reference_band_document(&fixture_document)
        .expect("cluster-signature reference-band fixture should be valid");

    fixture_document
}

fn validate_cluster_signature_reference_band_document(
    fixture_document: &ClusterSignatureReferenceBandDocument,
) -> Result<(), String> {
    if fixture_document.schema_version != CLUSTER_SIGNATURE_REFERENCE_BANDS_SCHEMA_VERSION {
        return Err(format!(
            "unexpected cluster-signature reference-band schema_version: {}",
            fixture_document.schema_version
        ));
    }
    if fixture_document.band_set_id != CLUSTER_SIGNATURE_REFERENCE_BANDS_SET_ID {
        return Err(format!(
            "unexpected cluster-signature reference-band set id: {}",
            fixture_document.band_set_id
        ));
    }
    if fixture_document.band_set_version.trim().is_empty() {
        return Err("cluster-signature reference-band set version must not be empty".to_string());
    }
    if fixture_document.determinism_policy.trim().is_empty() {
        return Err("cluster-signature determinism policy must not be empty".to_string());
    }
    if fixture_document.change_log.is_empty() {
        return Err("cluster-signature reference-band change_log must not be empty".to_string());
    }

    validate_clustering_reference_bands(&fixture_document.clustering_regression)?;
    validate_scale_consistency_reference_bands(&fixture_document.scale_consistency_regression)?;

    Ok(())
}

fn validate_clustering_reference_bands(
    reference_bands: &ClusteringReferenceBands,
) -> Result<(), String> {
    if reference_bands.seed_count == 0 {
        return Err("clustering reference-band seed_count must be greater than zero".to_string());
    }
    if reference_bands.interval_count < 2 {
        return Err("clustering reference-band interval_count must be at least two".to_string());
    }
    if reference_bands.interval_duration_nanos <= 0 {
        return Err(
            "clustering reference-band interval_duration_nanos must be positive".to_string(),
        );
    }
    if !reference_bands.seasonality_factor.is_finite() || reference_bands.seasonality_factor < 0.0 {
        return Err(
            "clustering reference-band seasonality_factor must be finite and non-negative"
                .to_string(),
        );
    }
    if !reference_bands.news_intensity.is_finite() || reference_bands.news_intensity < 0.0 {
        return Err(
            "clustering reference-band news_intensity must be finite and non-negative".to_string(),
        );
    }

    validate_metric_band(
        "clustering.fano_factor_gap",
        &reference_bands.reference_bands.fano_factor_gap,
    )?;
    validate_metric_band(
        "clustering.lag_one_autocorrelation_gap",
        &reference_bands.reference_bands.lag_one_autocorrelation_gap,
    )?;
    validate_metric_band(
        "clustering.clustered_lag_one_autocorrelation",
        &reference_bands
            .reference_bands
            .clustered_lag_one_autocorrelation,
    )?;
    validate_metric_band(
        "clustering.clustered_non_grid_run_ratio",
        &reference_bands.reference_bands.clustered_non_grid_run_ratio,
    )?;

    Ok(())
}

fn validate_scale_consistency_reference_bands(
    reference_bands: &ScaleConsistencyReferenceBands,
) -> Result<(), String> {
    if reference_bands.seed_count == 0 {
        return Err(
            "scale-consistency reference-band seed_count must be greater than zero".to_string(),
        );
    }
    if reference_bands.total_duration_nanos <= 0 {
        return Err(
            "scale-consistency reference-band total_duration_nanos must be positive".to_string(),
        );
    }
    if reference_bands.fine_interval_duration_nanos <= 0 {
        return Err(
            "scale-consistency reference-band fine_interval_duration_nanos must be positive"
                .to_string(),
        );
    }

    if reference_bands.horizon_bands.is_empty() {
        return Err("scale-consistency reference-band horizon panel must not be empty".to_string());
    }

    let observed_horizon_labels: BTreeSet<&str> = reference_bands
        .horizon_bands
        .iter()
        .map(|horizon_reference_bands| horizon_reference_bands.horizon_label.as_str())
        .collect();

    for required_horizon_label in ["15m", "1h", "1d"] {
        if !observed_horizon_labels.contains(required_horizon_label) {
            return Err(format!(
                "scale-consistency reference-band horizon panel missing required horizon `{required_horizon_label}`"
            ));
        }
    }

    for horizon_reference_bands in &reference_bands.horizon_bands {
        if horizon_reference_bands.coarse_interval_duration_nanos <= 0 {
            return Err(format!(
                "scale-consistency horizon `{}` has non-positive coarse_interval_duration_nanos",
                horizon_reference_bands.horizon_label
            ));
        }

        validate_metric_band(
            "scale.relative_total_event_count_delta",
            &horizon_reference_bands.relative_total_event_count_delta,
        )?;
        validate_metric_band(
            "scale.absolute_buy_share_delta",
            &horizon_reference_bands.absolute_buy_share_delta,
        )?;
        validate_metric_band(
            "scale.absolute_sell_share_delta",
            &horizon_reference_bands.absolute_sell_share_delta,
        )?;
        validate_metric_band(
            "scale.absolute_log_sell_to_buy_ratio_delta",
            &horizon_reference_bands.absolute_log_sell_to_buy_ratio_delta,
        )?;
    }

    Ok(())
}

fn validate_metric_band(metric_id: &str, metric_band: &MetricBand) -> Result<(), String> {
    if !metric_band.lower_bound.is_finite() || !metric_band.upper_bound.is_finite() {
        return Err(format!(
            "metric band bounds must be finite for metric_id={metric_id}"
        ));
    }
    if metric_band.lower_bound > metric_band.upper_bound {
        return Err(format!(
            "metric band lower_bound must not exceed upper_bound for metric_id={metric_id}"
        ));
    }

    Ok(())
}

fn evaluate_clustering_reference_bands(
    reference_bands: &ClusteringMetricBands,
    baseline_metrics: ClusteringMetrics,
    clustered_metrics: ClusteringMetrics,
) -> Vec<BandViolation> {
    let fano_factor_gap = clustered_metrics.mean_fano_factor - baseline_metrics.mean_fano_factor;
    let lag_one_autocorrelation_gap = clustered_metrics.mean_lag_one_autocorrelation
        - baseline_metrics.mean_lag_one_autocorrelation;

    let mut band_violations: Vec<BandViolation> = Vec::new();

    push_band_violation_if_outside(
        &mut band_violations,
        "clustering",
        None,
        "fano_factor_gap",
        fano_factor_gap,
        &reference_bands.fano_factor_gap,
    );
    push_band_violation_if_outside(
        &mut band_violations,
        "clustering",
        None,
        "lag_one_autocorrelation_gap",
        lag_one_autocorrelation_gap,
        &reference_bands.lag_one_autocorrelation_gap,
    );
    push_band_violation_if_outside(
        &mut band_violations,
        "clustering",
        None,
        "clustered_lag_one_autocorrelation",
        clustered_metrics.mean_lag_one_autocorrelation,
        &reference_bands.clustered_lag_one_autocorrelation,
    );
    push_band_violation_if_outside(
        &mut band_violations,
        "clustering",
        None,
        "clustered_non_grid_run_ratio",
        clustered_metrics.non_grid_run_ratio,
        &reference_bands.clustered_non_grid_run_ratio,
    );

    band_violations
}

fn evaluate_scale_consistency_reference_bands(
    horizon_reference_bands: &ScaleConsistencyHorizonReferenceBands,
    scale_consistency_metrics: ScaleConsistencyMetrics,
) -> Vec<BandViolation> {
    let mut band_violations: Vec<BandViolation> = Vec::new();

    let horizon_label = Some(horizon_reference_bands.horizon_label.as_str());

    push_band_violation_if_outside(
        &mut band_violations,
        "scale_consistency",
        horizon_label,
        "relative_total_event_count_delta",
        scale_consistency_metrics.relative_total_event_count_delta,
        &horizon_reference_bands.relative_total_event_count_delta,
    );
    push_band_violation_if_outside(
        &mut band_violations,
        "scale_consistency",
        horizon_label,
        "absolute_buy_share_delta",
        scale_consistency_metrics.absolute_buy_share_delta,
        &horizon_reference_bands.absolute_buy_share_delta,
    );
    push_band_violation_if_outside(
        &mut band_violations,
        "scale_consistency",
        horizon_label,
        "absolute_sell_share_delta",
        scale_consistency_metrics.absolute_sell_share_delta,
        &horizon_reference_bands.absolute_sell_share_delta,
    );
    push_band_violation_if_outside(
        &mut band_violations,
        "scale_consistency",
        horizon_label,
        "absolute_log_sell_to_buy_ratio_delta",
        scale_consistency_metrics.absolute_log_sell_to_buy_ratio_delta,
        &horizon_reference_bands.absolute_log_sell_to_buy_ratio_delta,
    );

    band_violations
}

fn push_band_violation_if_outside(
    band_violations: &mut Vec<BandViolation>,
    panel_id: &'static str,
    horizon_label: Option<&str>,
    metric_id: &'static str,
    observed_value: f64,
    metric_band: &MetricBand,
) {
    if observed_value >= metric_band.lower_bound && observed_value <= metric_band.upper_bound {
        return;
    }

    let delta_relative_to_band = if observed_value < metric_band.lower_bound {
        observed_value - metric_band.lower_bound
    } else {
        observed_value - metric_band.upper_bound
    };

    band_violations.push(BandViolation {
        panel_id,
        horizon_label: horizon_label.map(ToOwned::to_owned),
        metric_id,
        observed_value,
        lower_bound: metric_band.lower_bound,
        upper_bound: metric_band.upper_bound,
        delta_relative_to_band,
    });
}

fn assert_no_band_violations(
    band_violations: &[BandViolation],
    panel_label: &str,
    test_name: &str,
    invariant_ids: &[&str],
) {
    if band_violations.is_empty() {
        return;
    }

    let diagnostic_payload =
        build_band_violation_diagnostic_payload(test_name, invariant_ids, band_violations);

    panic!(
        "{panel_label} reference-band regression failed with {} violations:\n{diagnostic_payload}",
        band_violations.len(),
    );
}

fn build_band_violation_diagnostic_payload(
    test_name: &str,
    invariant_ids: &[&str],
    band_violations: &[BandViolation],
) -> String {
    let violations: Vec<HawkesFailureDiagnosticViolation> = band_violations
        .iter()
        .map(|band_violation| HawkesFailureDiagnosticViolation {
            category: classify_statistical_violation_category(
                band_violation.panel_id,
                band_violation.metric_id,
            ),
            scope: format!(
                "panel={} horizon={} metric={}",
                band_violation.panel_id,
                band_violation.horizon_label.as_deref().unwrap_or("n/a"),
                band_violation.metric_id
            ),
            detail: format!(
                "observed={} reference_band=[{}, {}] delta_relative_to_band={}",
                band_violation.observed_value,
                band_violation.lower_bound,
                band_violation.upper_bound,
                band_violation.delta_relative_to_band
            ),
            observed_value: Some(band_violation.observed_value),
            reference_lower_bound: Some(band_violation.lower_bound),
            reference_upper_bound: Some(band_violation.upper_bound),
            delta_relative_to_reference: Some(band_violation.delta_relative_to_band),
        })
        .collect();

    let report = build_hawkes_failure_diagnostic_report(
        "hawkes_cluster_signature_statistical_regression",
        test_name,
        HawkesInvariantCheckClass::StatisticalProperty,
        invariant_ids,
        violations,
    );

    render_hawkes_failure_diagnostic_report(&report)
}

fn compute_clustering_metrics_from_interval_count_runs(
    interval_count_runs: &[Vec<usize>],
    non_grid_run_flags: &[bool],
) -> ClusteringMetrics {
    assert_eq!(interval_count_runs.len(), non_grid_run_flags.len());

    let fano_factors: Vec<f64> = interval_count_runs
        .iter()
        .map(|interval_event_counts| calculate_fano_factor(interval_event_counts))
        .collect();
    let lag_one_autocorrelations: Vec<f64> = interval_count_runs
        .iter()
        .map(|interval_event_counts| calculate_lag_one_autocorrelation(interval_event_counts))
        .collect();
    let non_grid_run_count = non_grid_run_flags
        .iter()
        .filter(|is_non_grid_run| **is_non_grid_run)
        .count();

    ClusteringMetrics {
        mean_fano_factor: calculate_mean_f64(&fano_factors),
        mean_lag_one_autocorrelation: calculate_mean_f64(&lag_one_autocorrelations),
        non_grid_run_ratio: non_grid_run_count as f64 / interval_count_runs.len() as f64,
    }
}

fn compute_batch_clustering_metrics(
    seed_values: &[u64],
    kernel_parameters: HawkesKernelParameters,
    interval_count: usize,
    interval_duration_nanos: i64,
    seasonality_factor: f64,
    news_intensity: f64,
) -> ClusteringMetrics {
    let mut fano_factors: Vec<f64> = Vec::with_capacity(seed_values.len());
    let mut lag_one_autocorrelations: Vec<f64> = Vec::with_capacity(seed_values.len());
    let mut non_grid_run_count: usize = 0;

    for seed in seed_values {
        let run_result = run_seeded_tick_event_simulation(build_run_config(
            *seed,
            interval_duration_nanos,
            interval_count,
            kernel_parameters,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            seasonality_factor,
            news_intensity,
        ))
        .expect("batch clustering run should complete");
        let projected_event_path = run_result.projected_event_path();

        let interval_event_counts = collect_interval_event_counts(
            &projected_event_path,
            START_TIME_UTC_NANOS,
            interval_duration_nanos,
            interval_count,
        );

        fano_factors.push(calculate_fano_factor(&interval_event_counts));
        lag_one_autocorrelations.push(calculate_lag_one_autocorrelation(&interval_event_counts));

        if has_non_grid_event_time(
            &projected_event_path,
            START_TIME_UTC_NANOS,
            interval_duration_nanos,
        ) {
            non_grid_run_count = non_grid_run_count.saturating_add(1);
        }
    }

    ClusteringMetrics {
        mean_fano_factor: calculate_mean_f64(&fano_factors),
        mean_lag_one_autocorrelation: calculate_mean_f64(&lag_one_autocorrelations),
        non_grid_run_ratio: non_grid_run_count as f64 / seed_values.len() as f64,
    }
}

fn collect_scale_consistency_summaries(
    seed_values: &[u64],
    kernel_parameters: HawkesKernelParameters,
    total_duration_nanos: i64,
    fine_interval_duration_nanos: i64,
    coarse_interval_duration_nanos: i64,
    seasonality_factor: f64,
    news_intensity: f64,
) -> (Vec<HorizonSummary>, Vec<HorizonSummary>) {
    let fine_interval_count = usize::try_from(total_duration_nanos / fine_interval_duration_nanos)
        .expect("fine interval count should fit in usize");
    let coarse_interval_count =
        usize::try_from(total_duration_nanos / coarse_interval_duration_nanos)
            .expect("coarse interval count should fit in usize");

    let mut aggregated_summaries: Vec<HorizonSummary> = Vec::with_capacity(seed_values.len());
    let mut direct_summaries: Vec<HorizonSummary> = Vec::with_capacity(seed_values.len());

    for seed in seed_values {
        let fine_run_result = run_seeded_tick_event_simulation(build_run_config(
            *seed,
            fine_interval_duration_nanos,
            fine_interval_count,
            kernel_parameters,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            seasonality_factor,
            news_intensity,
        ))
        .expect("fine run should complete");

        let direct_coarse_run_result = run_seeded_tick_event_simulation(build_run_config(
            *seed,
            coarse_interval_duration_nanos,
            coarse_interval_count,
            kernel_parameters,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            seasonality_factor,
            news_intensity,
        ))
        .expect("direct coarse run should complete");
        let fine_event_path = fine_run_result.projected_event_path();
        let direct_coarse_event_path = direct_coarse_run_result.projected_event_path();

        aggregated_summaries.push(summarize_event_path(
            &fine_event_path,
            START_TIME_UTC_NANOS,
            coarse_interval_duration_nanos,
            coarse_interval_count,
        ));
        direct_summaries.push(summarize_event_path(
            &direct_coarse_event_path,
            START_TIME_UTC_NANOS,
            coarse_interval_duration_nanos,
            coarse_interval_count,
        ));
    }

    (aggregated_summaries, direct_summaries)
}

fn summarize_event_path(
    event_path: &[SeededTickEventRecord],
    start_time_utc_nanos: i64,
    horizon_duration_nanos: i64,
    horizon_interval_count: usize,
) -> HorizonSummary {
    let interval_buy_event_counts = collect_interval_buy_event_counts(
        event_path,
        start_time_utc_nanos,
        horizon_duration_nanos,
        horizon_interval_count,
    );

    let total_event_count: usize = interval_buy_event_counts
        .iter()
        .map(|(buy_event_count, sell_event_count)| buy_event_count + sell_event_count)
        .sum();
    let buy_event_count: usize = interval_buy_event_counts
        .iter()
        .map(|(buy_event_count, _sell_event_count)| *buy_event_count)
        .sum();
    let sell_event_count = total_event_count.saturating_sub(buy_event_count);

    let (buy_share, sell_share, log_sell_to_buy_ratio) = if total_event_count == 0 {
        (0.0, 0.0, 0.0)
    } else {
        (
            buy_event_count as f64 / total_event_count as f64,
            sell_event_count as f64 / total_event_count as f64,
            ((sell_event_count as f64 + 1.0) / (buy_event_count as f64 + 1.0)).ln(),
        )
    };

    HorizonSummary {
        mean_total_event_count: total_event_count as f64,
        mean_buy_share: buy_share,
        mean_sell_share: sell_share,
        mean_log_sell_to_buy_ratio: log_sell_to_buy_ratio,
    }
}

fn compute_scale_consistency_metrics(
    aggregated_summaries: &[HorizonSummary],
    direct_summaries: &[HorizonSummary],
) -> ScaleConsistencyMetrics {
    let aggregated_summary = HorizonSummary {
        mean_total_event_count: calculate_mean_f64(
            &aggregated_summaries
                .iter()
                .map(|summary| summary.mean_total_event_count)
                .collect::<Vec<f64>>(),
        ),
        mean_buy_share: calculate_mean_f64(
            &aggregated_summaries
                .iter()
                .map(|summary| summary.mean_buy_share)
                .collect::<Vec<f64>>(),
        ),
        mean_sell_share: calculate_mean_f64(
            &aggregated_summaries
                .iter()
                .map(|summary| summary.mean_sell_share)
                .collect::<Vec<f64>>(),
        ),
        mean_log_sell_to_buy_ratio: calculate_mean_f64(
            &aggregated_summaries
                .iter()
                .map(|summary| summary.mean_log_sell_to_buy_ratio)
                .collect::<Vec<f64>>(),
        ),
    };

    let direct_summary = HorizonSummary {
        mean_total_event_count: calculate_mean_f64(
            &direct_summaries
                .iter()
                .map(|summary| summary.mean_total_event_count)
                .collect::<Vec<f64>>(),
        ),
        mean_buy_share: calculate_mean_f64(
            &direct_summaries
                .iter()
                .map(|summary| summary.mean_buy_share)
                .collect::<Vec<f64>>(),
        ),
        mean_sell_share: calculate_mean_f64(
            &direct_summaries
                .iter()
                .map(|summary| summary.mean_sell_share)
                .collect::<Vec<f64>>(),
        ),
        mean_log_sell_to_buy_ratio: calculate_mean_f64(
            &direct_summaries
                .iter()
                .map(|summary| summary.mean_log_sell_to_buy_ratio)
                .collect::<Vec<f64>>(),
        ),
    };

    let relative_total_event_count_delta = if direct_summary.mean_total_event_count <= 0.0 {
        0.0
    } else {
        (aggregated_summary.mean_total_event_count - direct_summary.mean_total_event_count).abs()
            / direct_summary.mean_total_event_count
    };

    let absolute_buy_share_delta =
        (aggregated_summary.mean_buy_share - direct_summary.mean_buy_share).abs();
    let absolute_sell_share_delta =
        (aggregated_summary.mean_sell_share - direct_summary.mean_sell_share).abs();
    let absolute_log_sell_to_buy_ratio_delta = (aggregated_summary.mean_log_sell_to_buy_ratio
        - direct_summary.mean_log_sell_to_buy_ratio)
        .abs();

    ScaleConsistencyMetrics {
        aggregated_summary,
        direct_summary,
        relative_total_event_count_delta,
        absolute_buy_share_delta,
        absolute_sell_share_delta,
        absolute_log_sell_to_buy_ratio_delta,
    }
}

fn collect_interval_event_counts(
    event_path: &[SeededTickEventRecord],
    start_time_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_count: usize,
) -> Vec<usize> {
    let mut interval_event_counts: Vec<usize> = vec![0; interval_count];

    for event_record in event_path {
        let interval_position = event_interval_position(
            event_record.timestamp_utc_nanos,
            start_time_utc_nanos,
            interval_duration_nanos,
            interval_count,
        );

        if let Some(interval_position) = interval_position {
            interval_event_counts[interval_position] =
                interval_event_counts[interval_position].saturating_add(1);
        }
    }

    interval_event_counts
}

fn collect_interval_buy_event_counts(
    event_path: &[SeededTickEventRecord],
    start_time_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_count: usize,
) -> Vec<(usize, usize)> {
    let mut interval_side_event_counts: Vec<(usize, usize)> = vec![(0, 0); interval_count];

    for event_record in event_path {
        let interval_position = event_interval_position(
            event_record.timestamp_utc_nanos,
            start_time_utc_nanos,
            interval_duration_nanos,
            interval_count,
        );

        if let Some(interval_position) = interval_position {
            let (buy_event_count, sell_event_count) = interval_side_event_counts[interval_position];

            interval_side_event_counts[interval_position] = match event_record.side {
                SeededTickEventSide::Buy => (buy_event_count.saturating_add(1), sell_event_count),
                SeededTickEventSide::Sell => (buy_event_count, sell_event_count.saturating_add(1)),
            };
        }
    }

    interval_side_event_counts
}

fn event_interval_position(
    timestamp_utc_nanos: i64,
    start_time_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_count: usize,
) -> Option<usize> {
    if timestamp_utc_nanos < start_time_utc_nanos {
        return None;
    }

    let elapsed_nanos = timestamp_utc_nanos - start_time_utc_nanos;
    let interval_position = elapsed_nanos / interval_duration_nanos;

    if interval_position < 0 || interval_position >= interval_count as i64 {
        return None;
    }

    usize::try_from(interval_position).ok()
}

fn has_non_grid_event_time(
    event_path: &[SeededTickEventRecord],
    start_time_utc_nanos: i64,
    interval_duration_nanos: i64,
) -> bool {
    event_path.iter().any(|event_record| {
        let offset_nanos = event_record.timestamp_utc_nanos - start_time_utc_nanos;
        offset_nanos >= 0 && offset_nanos % interval_duration_nanos != 0
    })
}

fn calculate_fano_factor(interval_event_counts: &[usize]) -> f64 {
    if interval_event_counts.is_empty() {
        return 0.0;
    }

    let mean_event_count =
        interval_event_counts.iter().sum::<usize>() as f64 / interval_event_counts.len() as f64;
    if mean_event_count <= 0.0 {
        return 0.0;
    }

    let variance = interval_event_counts
        .iter()
        .map(|event_count| {
            let centered_count = *event_count as f64 - mean_event_count;
            centered_count * centered_count
        })
        .sum::<f64>()
        / interval_event_counts.len() as f64;

    variance / mean_event_count
}

fn calculate_lag_one_autocorrelation(interval_event_counts: &[usize]) -> f64 {
    if interval_event_counts.len() < 2 {
        return 0.0;
    }

    let left_interval_counts: Vec<f64> = interval_event_counts[..interval_event_counts.len() - 1]
        .iter()
        .map(|event_count| *event_count as f64)
        .collect();
    let right_interval_counts: Vec<f64> = interval_event_counts[1..]
        .iter()
        .map(|event_count| *event_count as f64)
        .collect();

    let left_mean = calculate_mean_f64(&left_interval_counts);
    let right_mean = calculate_mean_f64(&right_interval_counts);

    let numerator = left_interval_counts
        .iter()
        .zip(right_interval_counts.iter())
        .map(|(left_count, right_count)| (left_count - left_mean) * (right_count - right_mean))
        .sum::<f64>();

    let left_variance = left_interval_counts
        .iter()
        .map(|left_count| {
            let centered_left_count = left_count - left_mean;
            centered_left_count * centered_left_count
        })
        .sum::<f64>();
    let right_variance = right_interval_counts
        .iter()
        .map(|right_count| {
            let centered_right_count = right_count - right_mean;
            centered_right_count * centered_right_count
        })
        .sum::<f64>();

    let denominator = (left_variance * right_variance).sqrt();

    if denominator == 0.0 {
        return 0.0;
    }

    numerator / denominator
}

fn calculate_mean_f64(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    values.iter().sum::<f64>() / values.len() as f64
}

fn build_run_config(
    seed: u64,
    interval_duration_nanos: i64,
    interval_count: usize,
    kernel_parameters: HawkesKernelParameters,
    initial_excitation_state: HawkesExcitationState,
    seasonality_factor: f64,
    news_intensity: f64,
) -> SeededTickSimulationRunConfig {
    let hourly_factor_contract = build_uniform_factor_contract(
        interval_duration_nanos,
        interval_count,
        seasonality_factor,
        news_intensity,
    );

    SeededTickSimulationRunConfig::try_new(
        seed,
        DateTime::<Utc>::from_timestamp_nanos(START_TIME_UTC_NANOS),
        interval_count,
        SeededTickSimulationModelInputs {
            kernel_parameters,
            initial_excitation_state,
            mean_reversion_fitted_parameters:
                MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
                    .expect("zero-kappa endogenous mean reversion should construct"),
            mean_reversion_runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors::try_new(
                hourly_factor_contract.metadata.interval_alignment_policy,
                0.0,
                0.0,
                vec![0.0; interval_count],
                vec![0.0; interval_count],
                vec![interval_duration_nanos as f64 / NANOS_PER_HOUR as f64; interval_count],
            )
            .expect("disabled mean-reversion runtime anchors should construct"),
            initial_mean_reversion_boundary_state: MeanReversionBoundaryState::try_new(
                0.0, 0.0, 0.0, 0.0,
            )
            .expect("disabled mean-reversion boundary state should construct"),
            hourly_factor_contract,
            return_mark_hour_of_week_factors: vec![1.0; 24 * 7],
            observed_price_tick_size: None,
            calendar_registry: build_seeded_simulation_calendar_registry(),
            nig_shared_plus_shift_configuration: valid_nig_shared_plus_shift_configuration(),
            news_process_configuration: build_news_process_configuration(),
        },
        None,
    )
    .expect("seeded run config should initialize")
}

fn valid_nig_shared_plus_shift_configuration() -> NigSharedPlusShiftConfiguration {
    NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("nig configuration should construct")
}

fn build_news_process_configuration() -> NewsProcessConfiguration {
    NewsProcessConfiguration::try_new(
        NewsArrivalConfiguration {
            known: KnownNewsArrivalConfiguration {
                scheduled_event_timestamps_utc_nanos: Vec::new(),
            },
            surprise: SurpriseNewsArrivalConfiguration {
                baseline_intensity: 0.0,
            },
        },
        SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.5, 0.9)
                .expect("news gamma severity configuration should construct"),
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::try_new(
                0.4, 1.3, 1.1,
            )
            .expect("news base shock configuration should construct"),
            coupled_gap_marks: CoupledGapMarkConfiguration::new(0.75),
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 0.6,
            return_jump_scale: 1.2,
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 0.6,
            return_jump_scale: 1.2,
        },
    )
    .expect("news process configuration should construct")
}

fn build_uniform_factor_contract(
    interval_duration_nanos: i64,
    interval_count: usize,
    seasonality_factor: f64,
    news_intensity: f64,
) -> HourlyFactorContractOutput {
    let mut interval_indices: Vec<u64> = Vec::with_capacity(interval_count);
    let mut interval_start_utc_nanos: Vec<i64> = Vec::with_capacity(interval_count);
    let mut interval_end_utc_nanos: Vec<i64> = Vec::with_capacity(interval_count);
    let mut interval_start_exchange_local: Vec<String> = Vec::with_capacity(interval_count);
    let mut interval_end_exchange_local: Vec<String> = Vec::with_capacity(interval_count);

    for interval_position in 0..interval_count {
        let interval_start = START_TIME_UTC_NANOS
            + i64::try_from(interval_position).expect("interval position should fit in i64")
                * interval_duration_nanos;
        let interval_end = interval_start + interval_duration_nanos;

        interval_indices
            .push(u64::try_from(interval_position).expect("interval position should fit in u64"));
        interval_start_utc_nanos.push(interval_start);
        interval_end_utc_nanos.push(interval_end);
        interval_start_exchange_local
            .push(DateTime::<Utc>::from_timestamp_nanos(interval_start).to_rfc3339());
        interval_end_exchange_local
            .push(DateTime::<Utc>::from_timestamp_nanos(interval_end).to_rfc3339());
    }

    let request_end_utc_nanos = interval_end_utc_nanos
        .last()
        .copied()
        .expect("uniform factor contract should include at least one interval");

    HourlyFactorContractOutput {
        schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
        exchange_code: "XNYS".to_owned(),
        exchange_timezone_name: "America/New_York".to_owned(),
        request_start_utc_nanos: START_TIME_UTC_NANOS,
        request_end_utc_nanos,
        interval_duration_nanos,
        interval_indices,
        interval_start_utc_nanos: interval_start_utc_nanos.clone(),
        interval_end_utc_nanos: interval_end_utc_nanos.clone(),
        interval_start_exchange_local,
        interval_end_exchange_local,
        s_h: vec![seasonality_factor; interval_count],
        known_component_nu_h: vec![0.0; interval_count],
        surprise_component_nu_h: vec![news_intensity; interval_count],
        nu_h: vec![news_intensity; interval_count],
        metadata: HourlyFactorMaterializationMetadata {
            seasonality_materialization_method: DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD
                .to_owned(),
            news_intensity_materialization_method: DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD
                .to_owned(),
            interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
            timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
            spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
            fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
        },
        diagnostics: HourlyFactorContractDiagnostics {
            interval_count,
            interval_duration_nanos,
            first_interval_start_utc_nanos: interval_start_utc_nanos[0],
            last_interval_end_utc_nanos: request_end_utc_nanos,
            skipped_local_hour_interval_count: 0,
            repeated_local_hour_interval_count: 0,
        },
    }
}

fn build_seeded_simulation_calendar_registry() -> ExchangeCalendarRegistry {
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
                {
                  "weekday": "Monday",
                  "windows": [
                    {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                  ]
                },
                {
                  "weekday": "Tuesday",
                  "windows": [
                    {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                  ]
                },
                {
                  "weekday": "Wednesday",
                  "windows": [
                    {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                  ]
                },
                {
                  "weekday": "Thursday",
                  "windows": [
                    {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                  ]
                },
                {
                  "weekday": "Friday",
                  "windows": [
                    {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                  ]
                }
              ],
              "holiday_dates": ["2024-12-25"],
              "early_close_sessions": [
                {
                  "date": "2024-11-29",
                  "windows": [
                    {"label": "post_thanksgiving", "open_time": "09:30", "close_time": "13:00"}
                  ]
                }
              ]
            }
          ]
        }
        "#,
    )
    .expect("calendar registry fixture should load")
}

fn symmetric_kernel_parameters() -> HawkesKernelParameters {
    HawkesKernelParameters::try_new(
        0.9,
        1.6,
        HawkesKernelLoadings {
            plus_plus: 0.35,
            plus_minus: 0.20,
            minus_plus: 0.20,
            minus_minus: 0.35,
        },
        HawkesKernelLoadings {
            plus_plus: 0.02,
            plus_minus: 0.01,
            minus_plus: 0.01,
            minus_minus: 0.02,
        },
    )
    .expect("symmetric kernel parameters should be valid")
}

fn clustered_kernel_parameters() -> HawkesKernelParameters {
    HawkesKernelParameters::try_new(
        0.8,
        1.2,
        HawkesKernelLoadings {
            plus_plus: 0.75,
            plus_minus: 0.35,
            minus_plus: 0.32,
            minus_minus: 0.70,
        },
        HawkesKernelLoadings {
            plus_plus: 0.12,
            plus_minus: 0.05,
            minus_plus: 0.04,
            minus_minus: 0.10,
        },
    )
    .expect("clustered kernel parameters should be valid")
}

fn seed_values(start_seed: u64, seed_count: usize) -> Vec<u64> {
    (0..seed_count)
        .map(|seed_offset| start_seed + seed_offset as u64)
        .collect()
}
