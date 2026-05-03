//! Observer-aware execution entrypoints for validation workflows.

use std::fs;
use std::path::Path;

use janiform::calibration::market_data::normalize_market_data_source;
use janiform::validation::stylized_facts::{
    StylizedFactHorizonComparisonReportBundle, StylizedFactHorizonComparisonRequest,
};
use janiform_foundations::parallelism::resolved_cpu_count;

use super::artifacts::{
    LINEAGE_FILE_NAME, REPORT_FILE_STEM, cache_artifacts, generate_report_bundle,
    load_metric_catalog, validate_catalog_binding, write_cache_artifacts, write_json_artifact,
};
use super::empirical::{
    AnalysisWindow, ReturnObservation, analysis_window, infer_input_file_format, interval_count,
    observed_interval_count, parsed_intervals, prepare_empirical_input,
    prepare_empirical_plan_summary, tick_returns,
};
use super::metrics::{
    bootstrap_configuration_for_options, build_comparison_request, threshold_policy_for_options,
};
use super::output::{
    STYLIZED_FACTS_VALIDATION_PLAN_SCHEMA_VERSION, StylizedFactsValidationDecisionMode,
    StylizedFactsValidationExecutionMode, StylizedFactsValidationIntervalPlan,
    StylizedFactsValidationPlan, StylizedFactsValidationWorkflowOutput,
};
use super::progress::{StylizedFactsValidationProgressEvent, StylizedFactsValidationProgressPhase};
use super::request::StylizedFactsValidationWorkflowRequest;
use super::request_resolution::resolve_workflow_request_inputs;
use super::simulation::{derived_seeds, simulate_validation_paths};
use super::workflow::StylizedFactsValidationWorkflowObserver;
use crate::error::WorkflowError;

pub(super) const VALIDATION_PHASE_COUNT: usize = 6;
pub(super) const COMPUTE_METRICS_PHASE_STEP: usize = 4;

/// Build the resolved stylized-facts validation workflow plan.
pub fn build_stylized_facts_validation_plan(
    request: &StylizedFactsValidationWorkflowRequest,
) -> Result<StylizedFactsValidationPlan, WorkflowError> {
    let request = resolve_workflow_request_inputs(request)?;

    build_resolved_stylized_facts_validation_plan(&request)
}

fn build_resolved_stylized_facts_validation_plan(
    request: &StylizedFactsValidationWorkflowRequest,
) -> Result<StylizedFactsValidationPlan, WorkflowError> {
    validate_request(request)?;

    let execution_mode = execution_mode(request);
    let parsed_intervals = parsed_intervals(request)?;

    let empirical_summary = match (execution_mode, request.empirical_data_path.as_deref()) {
        (StylizedFactsValidationExecutionMode::RawData, Some(empirical_data_path)) => Some(
            prepare_empirical_plan_summary(request, empirical_data_path)?,
        ),
        _ => None,
    };

    let intervals = parsed_intervals
        .into_iter()
        .map(|interval| {
            let interval_count = empirical_summary.as_ref().and_then(|summary| {
                observed_interval_count(
                    &summary.return_observations,
                    summary.analysis_window.start_utc_nanos,
                    interval.duration_nanos,
                )
                .ok()
            });

            StylizedFactsValidationIntervalPlan {
                label: interval.label,
                duration_nanos: interval.duration_nanos,
                interval_count,
            }
        })
        .collect();

    let empirical_format = match request.empirical_data_path.as_deref() {
        Some(path) => Some(
            request
                .empirical_data_format
                .unwrap_or(infer_input_file_format(path)?),
        ),
        None => None,
    };

    let cache_artifacts = cache_artifacts(&request.output_dir, request.cache);

    let report_artifacts = vec![
        request
            .output_dir
            .join(format!("{REPORT_FILE_STEM}.report_bundle.json"))
            .display()
            .to_string(),
        request
            .output_dir
            .join(format!("{REPORT_FILE_STEM}.table_summary.json"))
            .display()
            .to_string(),
        request
            .output_dir
            .join(format!("{REPORT_FILE_STEM}.plot_payload.json"))
            .display()
            .to_string(),
        request
            .output_dir
            .join(LINEAGE_FILE_NAME)
            .display()
            .to_string(),
    ];

    Ok(StylizedFactsValidationPlan {
        schema_version: STYLIZED_FACTS_VALIDATION_PLAN_SCHEMA_VERSION,
        model_path: request
            .model_path
            .as_ref()
            .map(|path| path.display().to_string()),
        empirical_data_path: request
            .empirical_data_path
            .as_ref()
            .map(|path| path.display().to_string()),
        empirical_data_format: empirical_format.map(|format| format!("{format:?}").to_lowercase()),
        empirical_timestamp_column: empirical_summary
            .as_ref()
            .and_then(|summary| summary.timestamp_column.clone()),
        empirical_price_column: empirical_summary
            .as_ref()
            .and_then(|summary| summary.price_column.clone()),
        empirical_start_utc_nanos: empirical_summary
            .as_ref()
            .map(|summary| summary.analysis_window.start_utc_nanos),
        empirical_end_utc_nanos: empirical_summary
            .as_ref()
            .map(|summary| summary.analysis_window.end_utc_nanos),
        output_dir: request.output_dir.display().to_string(),
        intervals,
        simulation_path_count: request.simulation_path_count,
        seed: request.seed,
        cpu_count: resolved_cpu_count(request.cpu_count),
        generated_seeds: derived_seeds(request.seed, request.simulation_path_count),
        metric_families: vec!["tails_and_skew"],
        decision_mode: if request.comparison_request.is_some() || request.threshold_policy.is_some()
        {
            StylizedFactsValidationDecisionMode::ThresholdPolicy
        } else {
            StylizedFactsValidationDecisionMode::StatisticalBootstrap
        },
        cache_level: request.cache,
        cache_artifacts,
        report_artifacts,
        execution_mode,
    })
}

/// Run the stylized-facts validation workflow.
pub fn run_stylized_facts_validation_workflow(
    request: StylizedFactsValidationWorkflowRequest,
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
) -> Result<StylizedFactsValidationWorkflowOutput, WorkflowError> {
    let request = resolve_workflow_request_inputs(&request)?;

    let plan = build_resolved_stylized_facts_validation_plan(&request)?;

    let report_bundle = match plan.execution_mode {
        StylizedFactsValidationExecutionMode::PrecomputedComparison => {
            run_precomputed_comparison(&request, &plan)?
        }
        StylizedFactsValidationExecutionMode::RawData => {
            run_raw_data_validation(&request, &plan, observer)?
        }
    };

    Ok(StylizedFactsValidationWorkflowOutput {
        plan,
        report_bundle,
    })
}

fn run_precomputed_comparison(
    request: &StylizedFactsValidationWorkflowRequest,
    plan: &StylizedFactsValidationPlan,
) -> Result<StylizedFactHorizonComparisonReportBundle, WorkflowError> {
    let comparison_request_path =
        request
            .comparison_request
            .as_ref()
            .ok_or(WorkflowError::ValidationInputFailed {
                field: "--comparison-request",
                message: "missing precomputed comparison request".to_string(),
            })?;

    let metric_catalog = load_metric_catalog(request.metric_catalog.as_deref())?;
    let comparison_request: StylizedFactHorizonComparisonRequest =
        super::artifacts::load_typed_json_artifact(comparison_request_path)?;

    validate_catalog_binding(
        &metric_catalog,
        &comparison_request,
        comparison_request_path,
    )?;

    let report_bundle =
        generate_report_bundle(metric_catalog, comparison_request, &request.output_dir)?;

    write_json_artifact(&request.output_dir.join(LINEAGE_FILE_NAME), plan)?;

    Ok(report_bundle)
}

fn run_raw_data_validation(
    request: &StylizedFactsValidationWorkflowRequest,
    plan: &StylizedFactsValidationPlan,
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
) -> Result<StylizedFactHorizonComparisonReportBundle, WorkflowError> {
    let model_path = request
        .model_path
        .as_ref()
        .ok_or(WorkflowError::ValidationInputFailed {
            field: "--model-path",
            message: "missing model path".to_string(),
        })?;

    let empirical_data_path =
        request
            .empirical_data_path
            .as_ref()
            .ok_or(WorkflowError::ValidationInputFailed {
                field: "--empirical-data-path",
                message: "missing empirical data path".to_string(),
            })?;

    fs::create_dir_all(&request.output_dir).map_err(|source| {
        WorkflowError::ValidationArtifactWrite {
            path: request.output_dir.clone(),
            source,
        }
    })?;

    let (empirical_returns, analysis_window) =
        load_empirical_returns(request, empirical_data_path, observer)?;

    observe(
        observer,
        StylizedFactsValidationProgressPhase::DeriveIntervalReturns,
        "derive interval returns".to_string(),
        2,
    );

    let intervals = parsed_intervals(request)?;

    let runtime_interval_duration_nanos = intervals
        .iter()
        .map(|interval| interval.duration_nanos)
        .min()
        .ok_or(WorkflowError::ValidationInputFailed {
            field: "--intervals",
            message: "missing intervals".to_string(),
        })?;

    let simulated_interval_count = interval_count(
        analysis_window.start_utc_nanos,
        analysis_window.end_utc_nanos,
        runtime_interval_duration_nanos,
    )?;

    let simulated_paths = simulate_validation_paths(
        request,
        model_path,
        analysis_window,
        runtime_interval_duration_nanos,
        simulated_interval_count,
        observer,
    )?;

    let metric_catalog = load_metric_catalog(request.metric_catalog.as_deref())?;

    let threshold_policy = threshold_policy_for_options(request, &metric_catalog)?;

    let comparison_request = build_comparison_request(
        &metric_catalog,
        threshold_policy,
        &intervals,
        &empirical_returns,
        &simulated_paths.returns,
        bootstrap_configuration_for_options(request, &metric_catalog),
        resolved_cpu_count(request.cpu_count),
        observer,
    )?;

    observe(
        observer,
        StylizedFactsValidationProgressPhase::CompareMetrics,
        "compare metrics".to_string(),
        5,
    );

    let report_bundle = generate_report_bundle(
        metric_catalog,
        // Intentional copy because the report generator owns the request and
        // the cache writer persists the same generated request below.
        comparison_request.clone(),
        &request.output_dir,
    )?;

    observe(
        observer,
        StylizedFactsValidationProgressPhase::WriteArtifacts,
        "write artifacts".to_string(),
        6,
    );

    write_json_artifact(&request.output_dir.join(LINEAGE_FILE_NAME), plan)?;

    write_cache_artifacts(
        &request.output_dir,
        request.cache,
        &comparison_request,
        &simulated_paths.manifests,
        &simulated_paths.full_paths,
        plan,
    )?;

    Ok(report_bundle)
}

fn load_empirical_returns(
    request: &StylizedFactsValidationWorkflowRequest,
    empirical_data_path: &Path,
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
) -> Result<(Vec<ReturnObservation>, AnalysisWindow), WorkflowError> {
    observe(
        observer,
        StylizedFactsValidationProgressPhase::LoadEmpiricalData,
        "load empirical data".to_string(),
        1,
    );

    let empirical_input = prepare_empirical_input(request, empirical_data_path)?;

    let empirical_ticks = normalize_market_data_source(
        &empirical_input.source,
        &empirical_input.column_mapping,
        &empirical_input.normalization_policy,
        false,
    )
    .map_err(|source| WorkflowError::ValidationInputFailed {
        field: "--empirical-data-path",
        message: source.to_string(),
    })?;

    let empirical_returns = tick_returns(&empirical_ticks.tick_records)?;
    let analysis_window = analysis_window(&empirical_returns)?;

    Ok((empirical_returns, analysis_window))
}

fn validate_request(request: &StylizedFactsValidationWorkflowRequest) -> Result<(), WorkflowError> {
    if request.comparison_request.is_some() {
        if request.empirical_data_path.is_some()
            || request.model_path.is_some()
            || !request.intervals.is_empty()
        {
            return Err(WorkflowError::ValidationInputFailed {
                field: "--comparison-request",
                message: "precomputed comparison mode cannot also specify raw-data inputs"
                    .to_string(),
            });
        }

        return Ok(());
    }

    if request.model_path.is_none() {
        return Err(WorkflowError::ValidationMissingRequired {
            field: "--model-path",
        });
    }

    if request.empirical_data_path.is_none() {
        return Err(WorkflowError::ValidationMissingRequired {
            field: "--empirical-data-path",
        });
    }

    if request.intervals.is_empty() {
        return Err(WorkflowError::ValidationMissingRequired {
            field: "--intervals",
        });
    }

    if request.simulation_path_count == 0 {
        return Err(WorkflowError::ValidationInputFailed {
            field: "--simulation-path-count",
            message: "must be greater than 0".to_string(),
        });
    }

    if request.cpu_count == Some(0) {
        return Err(WorkflowError::ValidationInputFailed {
            field: "--cpu-count",
            message: "must be greater than 0".to_string(),
        });
    }

    if request.threshold_policy.is_some() && request.significance_level.is_some() {
        return Err(WorkflowError::ValidationInputFailed {
            field: "--significance-level",
            message: "significance level is only valid in statistical bootstrap mode".to_string(),
        });
    }

    if let Some(significance_level) = request.significance_level {
        if !significance_level.is_finite() || significance_level <= 0.0 || significance_level >= 1.0
        {
            return Err(WorkflowError::ValidationInputFailed {
                field: "--significance-level",
                message: "must be finite and in (0, 1)".to_string(),
            });
        }
    }

    Ok(())
}

fn execution_mode(
    request: &StylizedFactsValidationWorkflowRequest,
) -> StylizedFactsValidationExecutionMode {
    if request.comparison_request.is_some() {
        StylizedFactsValidationExecutionMode::PrecomputedComparison
    } else {
        StylizedFactsValidationExecutionMode::RawData
    }
}

fn observe(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    phase: StylizedFactsValidationProgressPhase,
    label: String,
    step: usize,
) {
    observer(StylizedFactsValidationProgressEvent {
        phase,
        label,
        step,
        total_steps: VALIDATION_PHASE_COUNT,
        completed_units: None,
        total_units: None,
        work_item_label: None,
        work_item_progress_percent: None,
        planned_work_item_labels: Vec::new(),
        work_item_columns: Vec::new(),
        work_item_metrics: Vec::new(),
        latest_signal: None,
    });
}
