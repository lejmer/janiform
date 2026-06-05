//! Tail-heaviness and skew diagnostics for stylized-fact validation.

use std::collections::BTreeSet;
use std::sync::mpsc;

use janiform_foundations::parallelism::resolved_cpu_count;
use rayon::ThreadPoolBuilder;
use rayon::join;
use rayon::prelude::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

use super::super::diagnostics_shared::{
    BlockBootstrapConfiguration, compute_block_bootstrap_confidence_interval, coverage_ratio,
    derive_metric_bootstrap_seed, validate_block_bootstrap_configuration,
};
use super::super::metric_catalog::STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE;
use super::super::time_buckets::{
    TimestampAnalysisWindow, interval_count_for_window, interval_position_for_timestamp,
};

use super::contracts::{
    TAIL_SKEW_DIAGNOSTICS_BOOTSTRAP_POLICY, TAIL_SKEW_DIAGNOSTICS_DETERMINISM_POLICY,
    TAIL_SKEW_DIAGNOSTICS_REPORT_SCHEMA_VERSION, TailSkewBlockBootstrapConfiguration,
    TailSkewConfidenceInterval, TailSkewDatasetInput, TailSkewDatasetMetricResult,
    TailSkewDatasetRole, TailSkewDiagnosticsError, TailSkewDiagnosticsReport,
    TailSkewDiagnosticsRequest, TailSkewHorizonBucket, TailSkewHorizonDiagnostics,
    TailSkewMetricClass, TailSkewMetricComparison, TailSkewMetricSampleSufficiencyPolicy,
    TailSkewMetricStatus, TailSkewObservedCounts, TailSkewReturnObservation,
    TailSkewSampleCoverage, TailSkewSampleSufficiencyPolicy,
};

use super::metrics::{
    TAIL_SKEW_METRIC_SPECIFICATIONS, TailSkewMetricComputation, TailSkewMetricSpecification,
};
use super::progress::{TailSkewDiagnosticsWorkerMessage, emit_dataset_metric_completed};

pub fn run_tail_skew_diagnostics(
    request: TailSkewDiagnosticsRequest,
) -> Result<TailSkewDiagnosticsReport, TailSkewDiagnosticsError> {
    run_tail_skew_diagnostics_internal(request, None)
}

pub(super) fn run_tail_skew_diagnostics_internal(
    request: TailSkewDiagnosticsRequest,
    progress_sender: Option<mpsc::Sender<TailSkewDiagnosticsWorkerMessage>>,
) -> Result<TailSkewDiagnosticsReport, TailSkewDiagnosticsError> {
    validate_request(&request)?;

    let analysis_window = TimestampAnalysisWindow::new(
        request.analysis_start_utc_nanos,
        request.analysis_end_utc_nanos,
    );

    let empirical_dataset =
        prepare_dataset("empirical", &request.empirical_dataset, analysis_window)?;
    let simulated_dataset =
        prepare_dataset("simulated", &request.simulated_dataset, analysis_window)?;

    let cpu_count = resolved_cpu_count(request.cpu_count);

    let horizon_reports = build_horizon_reports(TailSkewHorizonReportBuildInput {
        horizon_buckets: &request.horizon_buckets,
        empirical_dataset: &empirical_dataset,
        simulated_dataset: &simulated_dataset,
        analysis_window,
        sample_sufficiency_policy: request.sample_sufficiency_policy,
        bootstrap_configuration: request.bootstrap_configuration,
        cpu_count,
        progress_sender,
    })?;

    Ok(TailSkewDiagnosticsReport {
        schema_version: TAIL_SKEW_DIAGNOSTICS_REPORT_SCHEMA_VERSION,
        determinism_policy: TAIL_SKEW_DIAGNOSTICS_DETERMINISM_POLICY,
        bootstrap_policy: TAIL_SKEW_DIAGNOSTICS_BOOTSTRAP_POLICY,
        insufficient_data_status_code: STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE,
        empirical_dataset_id: empirical_dataset.dataset_id,
        simulated_dataset_id: simulated_dataset.dataset_id,
        analysis_start_utc_nanos: request.analysis_start_utc_nanos,
        analysis_end_utc_nanos: request.analysis_end_utc_nanos,
        horizons: horizon_reports,
    })
}

#[derive(Clone)]
struct TailSkewHorizonReportBuildInput<'a> {
    horizon_buckets: &'a [TailSkewHorizonBucket],
    empirical_dataset: &'a PreparedTailSkewDataset,
    simulated_dataset: &'a PreparedTailSkewDataset,
    analysis_window: TimestampAnalysisWindow,
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
    cpu_count: usize,
    progress_sender: Option<mpsc::Sender<TailSkewDiagnosticsWorkerMessage>>,
}

fn build_horizon_reports(
    input: TailSkewHorizonReportBuildInput<'_>,
) -> Result<Vec<TailSkewHorizonDiagnostics>, TailSkewDiagnosticsError> {
    let TailSkewHorizonReportBuildInput {
        horizon_buckets,
        empirical_dataset,
        simulated_dataset,
        analysis_window,
        sample_sufficiency_policy,
        bootstrap_configuration,
        cpu_count,
        progress_sender,
    } = input;

    if cpu_count <= 1 {
        return Ok(build_horizon_reports_serial(
            TailSkewHorizonReportSerialBuildInput {
                horizon_buckets,
                empirical_dataset,
                simulated_dataset,
                analysis_window,
                sample_sufficiency_policy,
                bootstrap_configuration,
                parallel_metric_work: false,
                progress_sender,
            },
        ));
    }

    let thread_pool = ThreadPoolBuilder::new()
        .num_threads(cpu_count)
        .build()
        .map_err(
            |source| TailSkewDiagnosticsError::InvalidParallelismConfiguration {
                details: source.to_string(),
            },
        )?;

    Ok(thread_pool.install(|| {
        if horizon_buckets.len() > 1 {
            horizon_buckets
                .par_iter()
                .enumerate()
                .map(|(horizon_position, horizon_bucket)| {
                    build_horizon_diagnostics(TailSkewHorizonDiagnosticsBuildInput {
                        horizon_position,
                        horizon_bucket,
                        empirical_dataset,
                        simulated_dataset,
                        analysis_window,
                        sample_sufficiency_policy,
                        bootstrap_configuration,
                        parallel_metric_work: true,
                        progress_sender: progress_sender.clone(),
                    })
                })
                .collect()
        } else {
            build_horizon_reports_serial(TailSkewHorizonReportSerialBuildInput {
                horizon_buckets,
                empirical_dataset,
                simulated_dataset,
                analysis_window,
                sample_sufficiency_policy,
                bootstrap_configuration,
                parallel_metric_work: true,
                progress_sender,
            })
        }
    }))
}

#[derive(Clone)]
struct TailSkewHorizonReportSerialBuildInput<'a> {
    horizon_buckets: &'a [TailSkewHorizonBucket],
    empirical_dataset: &'a PreparedTailSkewDataset,
    simulated_dataset: &'a PreparedTailSkewDataset,
    analysis_window: TimestampAnalysisWindow,
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
    parallel_metric_work: bool,
    progress_sender: Option<mpsc::Sender<TailSkewDiagnosticsWorkerMessage>>,
}

fn build_horizon_reports_serial(
    input: TailSkewHorizonReportSerialBuildInput<'_>,
) -> Vec<TailSkewHorizonDiagnostics> {
    let TailSkewHorizonReportSerialBuildInput {
        horizon_buckets,
        empirical_dataset,
        simulated_dataset,
        analysis_window,
        sample_sufficiency_policy,
        bootstrap_configuration,
        parallel_metric_work,
        progress_sender,
    } = input;

    horizon_buckets
        .iter()
        .enumerate()
        .map(|(horizon_position, horizon_bucket)| {
            build_horizon_diagnostics(TailSkewHorizonDiagnosticsBuildInput {
                horizon_position,
                horizon_bucket,
                empirical_dataset,
                simulated_dataset,
                analysis_window,
                sample_sufficiency_policy,
                bootstrap_configuration,
                parallel_metric_work,
                progress_sender: progress_sender.clone(),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedTailSkewDataset {
    dataset_id: String,
    return_observations_in_window: Vec<TailSkewReturnObservation>,
}

#[derive(Debug, Clone, PartialEq)]
struct HorizonReturnSeries {
    interval_count: usize,
    covered_interval_count: usize,
    interval_returns: Vec<f64>,
}

#[derive(Clone)]
struct TailSkewHorizonDiagnosticsBuildInput<'a> {
    horizon_position: usize,
    horizon_bucket: &'a TailSkewHorizonBucket,
    empirical_dataset: &'a PreparedTailSkewDataset,
    simulated_dataset: &'a PreparedTailSkewDataset,
    analysis_window: TimestampAnalysisWindow,
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
    parallel_metric_work: bool,
    progress_sender: Option<mpsc::Sender<TailSkewDiagnosticsWorkerMessage>>,
}

fn build_horizon_diagnostics(
    input: TailSkewHorizonDiagnosticsBuildInput<'_>,
) -> TailSkewHorizonDiagnostics {
    let TailSkewHorizonDiagnosticsBuildInput {
        horizon_position,
        horizon_bucket,
        empirical_dataset,
        simulated_dataset,
        analysis_window,
        sample_sufficiency_policy,
        bootstrap_configuration,
        parallel_metric_work,
        progress_sender,
    } = input;

    let empirical_horizon_series =
        build_horizon_return_series(empirical_dataset, analysis_window, horizon_bucket);
    let simulated_horizon_series =
        build_horizon_return_series(simulated_dataset, analysis_window, horizon_bucket);

    let sample_coverage_empirical = TailSkewSampleCoverage {
        horizon_interval_count: empirical_horizon_series.interval_count,
        covered_interval_count: empirical_horizon_series.covered_interval_count,
    };
    let sample_coverage_simulated = TailSkewSampleCoverage {
        horizon_interval_count: simulated_horizon_series.interval_count,
        covered_interval_count: simulated_horizon_series.covered_interval_count,
    };

    let metrics = if parallel_metric_work {
        TAIL_SKEW_METRIC_SPECIFICATIONS
            .par_iter()
            .map(|metric_specification| {
                build_metric_comparison_for_specification(TailSkewMetricComparisonBuildInput {
                    metric_specification: *metric_specification,
                    empirical_interval_returns: &empirical_horizon_series.interval_returns,
                    simulated_interval_returns: &simulated_horizon_series.interval_returns,
                    empirical_sample_coverage: sample_coverage_empirical,
                    simulated_sample_coverage: sample_coverage_simulated,
                    sample_sufficiency_policy,
                    bootstrap_configuration,
                    horizon_position,
                    horizon_label: horizon_bucket.horizon_label.as_str(),
                    parallel_dataset_work: true,
                    progress_sender: progress_sender.clone(),
                })
            })
            .collect()
    } else {
        TAIL_SKEW_METRIC_SPECIFICATIONS
            .iter()
            .map(|metric_specification| {
                build_metric_comparison_for_specification(TailSkewMetricComparisonBuildInput {
                    metric_specification: *metric_specification,
                    empirical_interval_returns: &empirical_horizon_series.interval_returns,
                    simulated_interval_returns: &simulated_horizon_series.interval_returns,
                    empirical_sample_coverage: sample_coverage_empirical,
                    simulated_sample_coverage: sample_coverage_simulated,
                    sample_sufficiency_policy,
                    bootstrap_configuration,
                    horizon_position,
                    horizon_label: horizon_bucket.horizon_label.as_str(),
                    parallel_dataset_work: false,
                    progress_sender: progress_sender.clone(),
                })
            })
            .collect()
    };

    TailSkewHorizonDiagnostics {
        horizon_label: horizon_bucket.horizon_label.clone(),
        horizon_duration_nanos: horizon_bucket.horizon_duration_nanos,
        metrics,
    }
}

#[derive(Clone)]
struct TailSkewMetricComparisonBuildInput<'a> {
    metric_specification: TailSkewMetricSpecification,
    empirical_interval_returns: &'a [f64],
    simulated_interval_returns: &'a [f64],
    empirical_sample_coverage: TailSkewSampleCoverage,
    simulated_sample_coverage: TailSkewSampleCoverage,
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
    horizon_position: usize,
    horizon_label: &'a str,
    parallel_dataset_work: bool,
    progress_sender: Option<mpsc::Sender<TailSkewDiagnosticsWorkerMessage>>,
}

fn build_metric_comparison_for_specification(
    input: TailSkewMetricComparisonBuildInput<'_>,
) -> TailSkewMetricComparison {
    let TailSkewMetricComparisonBuildInput {
        metric_specification,
        empirical_interval_returns,
        simulated_interval_returns,
        empirical_sample_coverage,
        simulated_sample_coverage,
        sample_sufficiency_policy,
        bootstrap_configuration,
        horizon_position,
        horizon_label,
        parallel_dataset_work,
        progress_sender,
    } = input;

    let required_minimums =
        (metric_specification.sample_sufficiency_selector)(sample_sufficiency_policy);

    let empirical_progress_sender = progress_sender.clone();
    let simulated_progress_sender = progress_sender;

    let empirical_work = || {
        let result = evaluate_metric(
            empirical_interval_returns,
            empirical_sample_coverage,
            required_minimums,
            bootstrap_configuration,
            derive_metric_bootstrap_seed(
                bootstrap_configuration.random_seed,
                horizon_position,
                1,
                metric_specification.metric_role,
            ),
            metric_specification.metric_evaluator,
        );

        emit_dataset_metric_completed(
            empirical_progress_sender.as_ref(),
            horizon_label,
            TailSkewDatasetRole::Empirical,
            metric_specification,
        );

        result
    };

    let simulated_work = || {
        let result = evaluate_metric(
            simulated_interval_returns,
            simulated_sample_coverage,
            required_minimums,
            bootstrap_configuration,
            derive_metric_bootstrap_seed(
                bootstrap_configuration.random_seed,
                horizon_position,
                2,
                metric_specification.metric_role,
            ),
            metric_specification.metric_evaluator,
        );

        emit_dataset_metric_completed(
            simulated_progress_sender.as_ref(),
            horizon_label,
            TailSkewDatasetRole::Simulated,
            metric_specification,
        );

        result
    };

    let (empirical, simulated) = if parallel_dataset_work {
        join(empirical_work, simulated_work)
    } else {
        (empirical_work(), simulated_work())
    };

    build_metric_comparison(
        metric_specification.metric_id,
        metric_specification.metric_name,
        metric_specification.metric_class,
        empirical,
        simulated,
    )
}

fn validate_request(request: &TailSkewDiagnosticsRequest) -> Result<(), TailSkewDiagnosticsError> {
    if request.analysis_start_utc_nanos >= request.analysis_end_utc_nanos {
        return Err(TailSkewDiagnosticsError::InvalidAnalysisWindow {
            analysis_start_utc_nanos: request.analysis_start_utc_nanos,
            analysis_end_utc_nanos: request.analysis_end_utc_nanos,
        });
    }

    validate_dataset_id("empirical", &request.empirical_dataset.dataset_id)?;
    validate_dataset_id("simulated", &request.simulated_dataset.dataset_id)?;

    if request.horizon_buckets.is_empty() {
        return Err(TailSkewDiagnosticsError::EmptyHorizonBucketSet);
    }

    let mut observed_horizon_labels: BTreeSet<&str> = BTreeSet::new();

    for (horizon_position, horizon_bucket) in request.horizon_buckets.iter().enumerate() {
        if horizon_bucket.horizon_label.trim().is_empty() {
            return Err(TailSkewDiagnosticsError::EmptyHorizonLabel { horizon_position });
        }

        if horizon_bucket.horizon_duration_nanos <= 0 {
            return Err(TailSkewDiagnosticsError::InvalidHorizonDuration {
                horizon_label: horizon_bucket.horizon_label.clone(),
                horizon_duration_nanos: horizon_bucket.horizon_duration_nanos,
            });
        }

        if !observed_horizon_labels.insert(horizon_bucket.horizon_label.as_str()) {
            return Err(TailSkewDiagnosticsError::DuplicateHorizonLabel {
                horizon_label: horizon_bucket.horizon_label.clone(),
            });
        }
    }

    validate_bootstrap_configuration(request.bootstrap_configuration)?;
    validate_sample_sufficiency_policy(request.sample_sufficiency_policy)?;

    validate_return_observations(
        "empirical",
        &request.empirical_dataset.dataset_id,
        &request.empirical_dataset.return_observations,
    )?;
    validate_return_observations(
        "simulated",
        &request.simulated_dataset.dataset_id,
        &request.simulated_dataset.return_observations,
    )?;

    Ok(())
}

fn validate_dataset_id(
    dataset_role: &'static str,
    dataset_id: &str,
) -> Result<(), TailSkewDiagnosticsError> {
    if dataset_id.trim().is_empty() {
        return Err(TailSkewDiagnosticsError::EmptyDatasetId { dataset_role });
    }

    Ok(())
}

fn validate_bootstrap_configuration(
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
) -> Result<(), TailSkewDiagnosticsError> {
    if let Err(field_name) = validate_block_bootstrap_configuration(block_bootstrap_configuration(
        bootstrap_configuration,
    )) {
        let details = match field_name {
            "confidence_level" => format!(
                "confidence_level must be finite and in (0, 1), found {}",
                bootstrap_configuration.confidence_level
            ),
            "replication_count" => "replication_count must be greater than 0".to_string(),
            "block_length_interval_count" => {
                "block_length_interval_count must be greater than 0".to_string()
            }
            _ => "invalid bootstrap configuration".to_string(),
        };

        return Err(TailSkewDiagnosticsError::InvalidBootstrapConfiguration {
            field_name,
            details,
        });
    }

    Ok(())
}

fn block_bootstrap_configuration(
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
) -> BlockBootstrapConfiguration {
    BlockBootstrapConfiguration {
        confidence_level: bootstrap_configuration.confidence_level,
        replication_count: bootstrap_configuration.replication_count,
        block_length_interval_count: bootstrap_configuration.block_length_interval_count,
    }
}

fn validate_sample_sufficiency_policy(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> Result<(), TailSkewDiagnosticsError> {
    sample_sufficiency_policy
        .returns_abs_quantile_shape_ratio_q99_q95
        .validate("returns_abs_quantile_shape_ratio_q99_q95")?;
    sample_sufficiency_policy
        .returns_left_tail_expected_shortfall_to_var_ratio
        .validate("returns_left_tail_expected_shortfall_to_var_ratio")?;
    sample_sufficiency_policy
        .returns_bowley_skewness_quantile
        .validate("returns_bowley_skewness_quantile")?;
    sample_sufficiency_policy
        .returns_signed_tail_expected_shortfall_imbalance
        .validate("returns_signed_tail_expected_shortfall_imbalance")?;
    sample_sufficiency_policy
        .returns_l_moment_lambda2
        .validate("returns_l_moment_lambda2")?;
    sample_sufficiency_policy
        .returns_l_moment_tau3
        .validate("returns_l_moment_tau3")?;
    sample_sufficiency_policy
        .returns_central_variance
        .validate("returns_central_variance")?;
    sample_sufficiency_policy
        .returns_central_skewness
        .validate("returns_central_skewness")?;
    sample_sufficiency_policy
        .returns_central_excess_kurtosis
        .validate("returns_central_excess_kurtosis")?;

    Ok(())
}

fn validate_return_observations(
    dataset_role: &'static str,
    dataset_id: &str,
    return_observations: &[TailSkewReturnObservation],
) -> Result<(), TailSkewDiagnosticsError> {
    for (observation_position, return_observation) in return_observations.iter().enumerate() {
        if !return_observation.return_increment.is_finite() {
            return Err(TailSkewDiagnosticsError::InvalidReturnObservation {
                dataset_role,
                dataset_id: dataset_id.to_string(),
                observation_position,
                return_increment: return_observation.return_increment,
            });
        }
    }

    Ok(())
}

fn prepare_dataset(
    dataset_role: &'static str,
    dataset: &TailSkewDatasetInput,
    analysis_window: TimestampAnalysisWindow,
) -> Result<PreparedTailSkewDataset, TailSkewDiagnosticsError> {
    validate_dataset_id(dataset_role, &dataset.dataset_id)?;
    validate_return_observations(
        dataset_role,
        &dataset.dataset_id,
        &dataset.return_observations,
    )?;

    let mut return_observations_in_window: Vec<TailSkewReturnObservation> = dataset
        .return_observations
        .iter()
        .copied()
        .filter(|return_observation| {
            analysis_window.contains_timestamp(return_observation.timestamp_utc_nanos)
        })
        .collect();
    return_observations_in_window
        .sort_by_key(|return_observation| return_observation.timestamp_utc_nanos);

    Ok(PreparedTailSkewDataset {
        dataset_id: dataset.dataset_id.clone(),
        return_observations_in_window,
    })
}

fn build_horizon_return_series(
    prepared_dataset: &PreparedTailSkewDataset,
    analysis_window: TimestampAnalysisWindow,
    horizon_bucket: &TailSkewHorizonBucket,
) -> HorizonReturnSeries {
    let interval_count =
        interval_count_for_window(analysis_window, horizon_bucket.horizon_duration_nanos)
            .unwrap_or(0);

    let mut interval_return_sum: Vec<f64> = vec![0.0; interval_count];
    let mut return_interval_has_observation: Vec<bool> = vec![false; interval_count];

    for return_observation in &prepared_dataset.return_observations_in_window {
        let Some(interval_position) = interval_position_for_timestamp(
            return_observation.timestamp_utc_nanos,
            analysis_window,
            horizon_bucket.horizon_duration_nanos,
            interval_count,
        ) else {
            continue;
        };

        interval_return_sum[interval_position] += return_observation.return_increment;
        return_interval_has_observation[interval_position] = true;
    }

    let covered_interval_count = return_interval_has_observation
        .iter()
        .filter(|covered| **covered)
        .count();

    let interval_returns: Vec<f64> = interval_return_sum
        .iter()
        .zip(return_interval_has_observation.iter())
        .filter_map(|(interval_return, has_observation)| {
            (*has_observation).then_some(*interval_return)
        })
        .collect();

    HorizonReturnSeries {
        interval_count,
        covered_interval_count,
        interval_returns,
    }
}

fn evaluate_metric<F>(
    interval_returns: &[f64],
    sample_coverage: TailSkewSampleCoverage,
    required_minimums: TailSkewMetricSampleSufficiencyPolicy,
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
    bootstrap_seed: u64,
    metric_evaluator: F,
) -> TailSkewDatasetMetricResult
where
    F: Fn(&[f64]) -> TailSkewMetricComputation,
{
    let computation = metric_evaluator(interval_returns);

    let observed_counts = TailSkewObservedCounts {
        sample_count: interval_returns.len(),
        effective_sample_count: computation.effective_sample_count,
        tail_observation_count: computation.tail_observation_count,
    };

    let horizon_coverage_ratio = coverage_ratio(
        sample_coverage.covered_interval_count,
        sample_coverage.horizon_interval_count,
    );

    let status = resolve_metric_status(
        computation.value,
        observed_counts,
        horizon_coverage_ratio,
        required_minimums,
    );

    let confidence_interval = if status == TailSkewMetricStatus::Available {
        computation.value.map(|point_estimate| {
            let (lower_bound, upper_bound) = compute_block_bootstrap_confidence_interval(
                interval_returns,
                point_estimate,
                block_bootstrap_configuration(bootstrap_configuration),
                bootstrap_seed,
                |resampled_returns| metric_evaluator(resampled_returns).value,
            );
            TailSkewConfidenceInterval {
                lower_bound,
                upper_bound,
                confidence_level: bootstrap_configuration.confidence_level,
            }
        })
    } else {
        None
    };

    TailSkewDatasetMetricResult {
        status,
        value: computation.value,
        confidence_interval,
        sample_coverage,
        observed_counts,
        required_minimums,
        horizon_coverage_ratio,
    }
}

fn build_metric_comparison(
    metric_id: &'static str,
    metric_name: &'static str,
    metric_class: TailSkewMetricClass,
    empirical: TailSkewDatasetMetricResult,
    simulated: TailSkewDatasetMetricResult,
) -> TailSkewMetricComparison {
    let absolute_delta = match (
        empirical.status,
        empirical.value,
        simulated.status,
        simulated.value,
    ) {
        (
            TailSkewMetricStatus::Available,
            Some(empirical_value),
            TailSkewMetricStatus::Available,
            Some(simulated_value),
        ) => Some((empirical_value - simulated_value).abs()),
        _ => None,
    };

    let status = if absolute_delta.is_some() {
        TailSkewMetricStatus::Available
    } else {
        TailSkewMetricStatus::InsufficientData
    };

    TailSkewMetricComparison {
        metric_id,
        metric_name,
        metric_class,
        status,
        empirical,
        simulated,
        absolute_delta,
    }
}

fn resolve_metric_status(
    value: Option<f64>,
    observed_counts: TailSkewObservedCounts,
    horizon_coverage_ratio: f64,
    required_minimums: TailSkewMetricSampleSufficiencyPolicy,
) -> TailSkewMetricStatus {
    let has_sufficient_counts = observed_counts.sample_count
        >= required_minimums.minimum_sample_count
        && observed_counts.effective_sample_count
            >= required_minimums.minimum_effective_sample_count
        && observed_counts.tail_observation_count
            >= required_minimums.minimum_tail_observation_count;

    let has_sufficient_coverage =
        horizon_coverage_ratio >= required_minimums.minimum_horizon_coverage_ratio;

    if value.is_some() && has_sufficient_counts && has_sufficient_coverage {
        TailSkewMetricStatus::Available
    } else {
        TailSkewMetricStatus::InsufficientData
    }
}
