use std::fs;
use std::path::Path;

use janiform::calibration::artifacts::CalibrationExecutionMode;
use janiform::simulation::SeededFinePathRecord;
use janiform::validation::stylized_facts::{
    StylizedFactHorizonComparisonArtifactExportConfig,
    StylizedFactHorizonComparisonReportGenerationError,
    StylizedFactHorizonComparisonReportGeneratorOptions,
    StylizedFactHorizonComparisonReportGeneratorRequest, StylizedFactHorizonComparisonRequest,
    StylizedFactMetricCatalog, StylizedFactObservedMetricInput, build_stylized_fact_metric_catalog,
    run_stylized_fact_horizon_comparison_report_generator,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::output::{StylizedFactsValidationCacheLevel, StylizedFactsValidationPlan};
use crate::error::WorkflowError;

pub(super) const DERIVED_COMPARISON_REQUEST_FILE_NAME: &str = "comparison_request.json";
pub(super) const LINEAGE_FILE_NAME: &str = "lineage.json";
pub(super) const SIMULATION_MANIFEST_FILE_NAME: &str = "simulation_manifest.json";
pub(super) const METRIC_OBSERVATIONS_FILE_NAME: &str = "metric_observations.json";
pub(super) const FULL_SIMULATION_PATHS_FILE_NAME: &str = "simulation_paths.json";
pub(super) const REPORT_FILE_STEM: &str = "stylized_facts";

pub(super) fn generate_report_bundle(
    metric_catalog: StylizedFactMetricCatalog,
    comparison_request: StylizedFactHorizonComparisonRequest,
    output_dir: &Path,
) -> Result<
    janiform::validation::stylized_facts::StylizedFactHorizonComparisonReportBundle,
    WorkflowError,
> {
    fs::create_dir_all(output_dir).map_err(|source| WorkflowError::ValidationArtifactWrite {
        path: output_dir.to_path_buf(),
        source,
    })?;

    run_stylized_fact_horizon_comparison_report_generator(
        StylizedFactHorizonComparisonReportGeneratorRequest {
            metric_catalog,
            comparison_request,
            generation_options: StylizedFactHorizonComparisonReportGeneratorOptions {
                execution_mode: CalibrationExecutionMode::Disk,
                artifact_export: Some(StylizedFactHorizonComparisonArtifactExportConfig {
                    export_directory: output_dir.display().to_string(),
                    file_stem: REPORT_FILE_STEM.to_string(),
                }),
            },
        },
    )
    .map_err(map_report_generation_error)
}

pub(super) fn load_metric_catalog(
    path: Option<&Path>,
) -> Result<StylizedFactMetricCatalog, WorkflowError> {
    match path {
        Some(path) => load_typed_json_artifact(path),
        None => Ok(build_stylized_fact_metric_catalog()),
    }
}

pub(super) fn validate_catalog_binding(
    metric_catalog: &StylizedFactMetricCatalog,
    comparison_request: &StylizedFactHorizonComparisonRequest,
    comparison_request_path: &Path,
) -> Result<(), WorkflowError> {
    if comparison_request.metric_catalog_id != metric_catalog.catalog_id {
        return Err(WorkflowError::ValidationArtifactMalformed {
            path: comparison_request_path.to_path_buf(),
            message: format!(
                "field `metric_catalog_id` must match metric catalog field `catalog_id`: expected `{}`, found `{}`",
                metric_catalog.catalog_id, comparison_request.metric_catalog_id
            ),
        });
    }

    if comparison_request.metric_catalog_version != metric_catalog.catalog_version {
        return Err(WorkflowError::ValidationArtifactMalformed {
            path: comparison_request_path.to_path_buf(),
            message: format!(
                "field `metric_catalog_version` must match metric catalog field `catalog_version`: expected `{}`, found `{}`",
                metric_catalog.catalog_version, comparison_request.metric_catalog_version
            ),
        });
    }

    Ok(())
}

pub(super) fn write_cache_artifacts(
    output_dir: &Path,
    cache_level: Option<StylizedFactsValidationCacheLevel>,
    comparison_request: &StylizedFactHorizonComparisonRequest,
    simulation_manifests: &[SimulationPathManifest],
    full_simulation_paths: &[FullSimulationPathCache],
    plan: &StylizedFactsValidationPlan,
) -> Result<(), WorkflowError> {
    let Some(cache_level) = cache_level else {
        return Ok(());
    };

    write_json_artifact(
        &output_dir.join(DERIVED_COMPARISON_REQUEST_FILE_NAME),
        comparison_request,
    )?;

    write_json_artifact(
        &output_dir.join(SIMULATION_MANIFEST_FILE_NAME),
        simulation_manifests,
    )?;

    let run_input = comparison_request.calibration_runs.first().ok_or(
        WorkflowError::ValidationInputFailed {
            field: "validate stylized-facts",
            message: "generated comparison request must contain one validation run".to_string(),
        },
    )?;

    write_json_artifact(
        &output_dir.join(METRIC_OBSERVATIONS_FILE_NAME),
        &MetricObservationCache {
            empirical_metrics: &run_input.empirical_metrics,
            simulated_metrics: &run_input.simulated_metrics,
        },
    )?;

    write_json_artifact(&output_dir.join(LINEAGE_FILE_NAME), plan)?;

    if cache_level == StylizedFactsValidationCacheLevel::Full {
        write_json_artifact(
            &output_dir.join(FULL_SIMULATION_PATHS_FILE_NAME),
            &FullSimulationCache {
                paths: full_simulation_paths,
            },
        )?;
    }

    Ok(())
}

pub(super) fn write_json_artifact<T: Serialize + ?Sized>(
    path: &Path,
    value: &T,
) -> Result<(), WorkflowError> {
    let payload = serde_json::to_vec_pretty(value)
        .map_err(|source| WorkflowError::OutputSerialize { source })?;

    fs::write(path, payload).map_err(|source| WorkflowError::ValidationArtifactWrite {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn cache_artifacts(
    output_dir: &Path,
    cache_level: Option<StylizedFactsValidationCacheLevel>,
) -> Vec<String> {
    let Some(cache_level) = cache_level else {
        return Vec::new();
    };

    let mut artifacts = vec![
        output_dir.join(DERIVED_COMPARISON_REQUEST_FILE_NAME),
        output_dir.join(SIMULATION_MANIFEST_FILE_NAME),
        output_dir.join(METRIC_OBSERVATIONS_FILE_NAME),
    ];

    if cache_level == StylizedFactsValidationCacheLevel::Full {
        artifacts.push(output_dir.join(FULL_SIMULATION_PATHS_FILE_NAME));
    }

    artifacts
        .into_iter()
        .map(|path| path.display().to_string())
        .collect()
}

fn map_report_generation_error(
    source: StylizedFactHorizonComparisonReportGenerationError,
) -> WorkflowError {
    WorkflowError::ValidationInputFailed {
        field: "validate stylized-facts",
        message: source.to_string(),
    }
}

pub(super) fn load_typed_json_artifact<T: DeserializeOwned>(
    path: &Path,
) -> Result<T, WorkflowError> {
    let payload = fs::read(path).map_err(|source| WorkflowError::ValidationArtifactRead {
        path: path.to_path_buf(),
        source,
    })?;

    serde_json::from_slice(&payload).map_err(|source| WorkflowError::ValidationArtifactParse {
        path: path.to_path_buf(),
        source,
    })
}

#[derive(Debug, Serialize)]
pub(super) struct SimulationPathManifest {
    pub(super) path_position: usize,
    pub(super) seed: u64,
    pub(super) transition_count: usize,
    pub(super) event_count: usize,
}

#[derive(Debug, Serialize)]
struct MetricObservationCache<'a> {
    empirical_metrics: &'a [StylizedFactObservedMetricInput],
    simulated_metrics: &'a [StylizedFactObservedMetricInput],
}

#[derive(Debug, Serialize)]
struct FullSimulationCache<'a> {
    paths: &'a [FullSimulationPathCache],
}

#[derive(Debug, Serialize)]
pub(super) struct FullSimulationPathCache {
    pub(super) path_position: usize,
    pub(super) seed: u64,
    pub(super) fine_path: Vec<SeededFinePathRecord>,
}
