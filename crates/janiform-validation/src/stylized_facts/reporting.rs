//! Stylized-fact horizon comparison report generation and table rows.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use janiform_calibration_artifacts::CalibrationExecutionMode;
use janiform_foundations::json::document::{JsonDocumentWriteError, write_pretty_json_document};
use janiform_foundations::strings::validate_non_empty_string;
use serde::{Deserialize, Serialize};

use super::comparison::{
    StylizedFactCalibrationRunComparisonReport, StylizedFactComparisonDecisionState,
    StylizedFactDatasetMetricStatus, StylizedFactDecisionReasonCode, StylizedFactHorizonComparison,
    StylizedFactHorizonComparisonBatchReport, StylizedFactHorizonComparisonError,
    StylizedFactHorizonComparisonRequest, StylizedFactMetricAlignmentStatus,
    StylizedFactMetricComparison, StylizedFactObservedCounts,
    StylizedFactSampleSufficiencyReasonCode, StylizedFactSampleSufficiencyRequirements,
    run_stylized_fact_horizon_comparison_engine,
};
use super::metric_catalog::{
    STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE, StylizedFactMetricCatalog,
    StylizedFactMetricCatalogValidationError, StylizedFactMetricClass,
};

pub const STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_SCHEMA_VERSION: &str =
    "validation.stylized_fact.interval_comparison.report_bundle.v1";
pub const STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_DETERMINISM_POLICY: &str =
    "stable_run_interval_metric_order_with_metric_catalog_classification.v1";
pub const STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "validation.stylized_fact.interval_comparison.report_bundle.output_schema_metadata.v1";
pub const STYLIZED_FACT_HORIZON_COMPARISON_TABLE_SUMMARY_SCHEMA_VERSION: &str =
    "validation.stylized_fact.interval_comparison.table_summary.v1";

const TABLE_SUMMARY_REQUIRED_FIELDS: [&str; 3] = ["schema_version", "determinism_policy", "rows"];
const TABLE_SUMMARY_ROW_REQUIRED_FIELDS: [&str; 24] = [
    "calibration_run_id",
    "empirical_dataset_id",
    "simulated_dataset_id",
    "interval_label",
    "metric_id",
    "metric_name",
    "metric_variant",
    "metric_class",
    "alignment_status",
    "status",
    "absolute_delta",
    "standardized_delta",
    "empirical_status",
    "simulated_status",
    "empirical_value",
    "simulated_value",
    "empirical_confidence_interval",
    "simulated_confidence_interval",
    "empirical_sample_coverage",
    "simulated_sample_coverage",
    "empirical_interval_coverage_ratio",
    "simulated_interval_coverage_ratio",
    "insufficient_data_reasons",
    "decision_reason_codes",
];
const REPORT_BUNDLE_REQUIRED_FIELDS: [&str; 6] = [
    "schema_version",
    "determinism_policy",
    "output_schema_metadata",
    "comparison_report",
    "table_summary",
    "artifact_export",
];
const REPORT_BUNDLE_METRIC_CLASS_VOCABULARY: [&str; 2] = ["primary_robust", "secondary_moment"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactHorizonComparisonReportGeneratorRequest {
    pub metric_catalog: StylizedFactMetricCatalog,
    pub comparison_request: StylizedFactHorizonComparisonRequest,
    pub generation_options: StylizedFactHorizonComparisonReportGeneratorOptions,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactHorizonComparisonReportGeneratorOptions {
    pub execution_mode: CalibrationExecutionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_export: Option<StylizedFactHorizonComparisonArtifactExportConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactHorizonComparisonArtifactExportConfig {
    pub export_directory: String,
    pub file_stem: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub required_bundle_fields: Vec<&'static str>,
    pub required_table_summary_fields: Vec<&'static str>,
    pub required_table_summary_row_fields: Vec<&'static str>,
    pub decision_state_vocabulary: Vec<&'static str>,
    pub metric_class_vocabulary: Vec<&'static str>,
    pub insufficient_data_status_code: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StylizedFactHorizonComparisonTableSummary {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub rows: Vec<StylizedFactHorizonComparisonTableSummaryRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StylizedFactHorizonComparisonTableSummaryRow {
    pub calibration_run_id: String,
    pub empirical_dataset_id: String,
    pub simulated_dataset_id: String,
    #[serde(rename = "interval_label")]
    pub horizon_label: String,
    pub metric_id: String,
    pub metric_name: String,
    pub metric_variant: Option<String>,
    pub metric_class: StylizedFactMetricClass,
    pub alignment_status: StylizedFactMetricAlignmentStatus,
    pub status: StylizedFactComparisonDecisionState,
    pub absolute_delta: Option<f64>,
    pub standardized_delta: Option<f64>,
    pub empirical_status: StylizedFactDatasetMetricStatus,
    pub simulated_status: StylizedFactDatasetMetricStatus,
    pub empirical_value: Option<f64>,
    pub simulated_value: Option<f64>,
    pub empirical_confidence_interval: Option<StylizedFactMetricIntervalBounds>,
    pub simulated_confidence_interval: Option<StylizedFactMetricIntervalBounds>,
    pub empirical_sample_coverage: StylizedFactMetricCoverageSummary,
    pub simulated_sample_coverage: StylizedFactMetricCoverageSummary,
    #[serde(rename = "empirical_interval_coverage_ratio")]
    pub empirical_horizon_coverage_ratio: f64,
    #[serde(rename = "simulated_interval_coverage_ratio")]
    pub simulated_horizon_coverage_ratio: f64,
    pub insufficient_data_reasons: StylizedFactMetricInsufficientDataReasons,
    pub decision_reason_codes: Vec<StylizedFactDecisionReasonCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct StylizedFactMetricIntervalBounds {
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub confidence_level: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StylizedFactMetricCoverageSummary {
    #[serde(rename = "interval_count")]
    pub horizon_interval_count: usize,
    pub covered_interval_count: usize,
    pub sample_count: usize,
    pub effective_sample_count: usize,
    pub tail_observation_count: usize,
    pub minimum_sample_count: usize,
    pub minimum_effective_sample_count: usize,
    pub minimum_tail_observation_count: usize,
    pub minimum_horizon_coverage_ratio_scaled_bps: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StylizedFactMetricInsufficientDataReasons {
    pub empirical: Vec<StylizedFactSampleSufficiencyReasonCode>,
    pub simulated: Vec<StylizedFactSampleSufficiencyReasonCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactHorizonComparisonArtifactKind {
    ReportBundle,
    TableSummary,
    PlotPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StylizedFactHorizonComparisonExportedArtifactFile {
    pub artifact_kind: StylizedFactHorizonComparisonArtifactKind,
    pub file_name: String,
    pub absolute_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StylizedFactHorizonComparisonArtifactExportReceipt {
    pub export_directory: String,
    pub file_stem: String,
    pub exported_files: Vec<StylizedFactHorizonComparisonExportedArtifactFile>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StylizedFactHorizonComparisonReportBundle {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub output_schema_metadata: StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata,
    pub comparison_report: StylizedFactHorizonComparisonBatchReport,
    pub table_summary: StylizedFactHorizonComparisonTableSummary,
    pub artifact_export: Option<StylizedFactHorizonComparisonArtifactExportReceipt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StylizedFactHorizonComparisonReportGenerationError {
    InvalidMetricCatalog {
        source: StylizedFactMetricCatalogValidationError,
    },
    MetricCatalogIdMismatch {
        expected_catalog_id: String,
        found_catalog_id: String,
    },
    MetricCatalogVersionMismatch {
        expected_catalog_version: String,
        found_catalog_version: String,
    },
    InvalidComparisonRequest {
        source: StylizedFactHorizonComparisonError,
    },
    MissingMetricCatalogEntry {
        metric_id: String,
    },
    ArtifactExportRequiresDiskMode {
        execution_mode: CalibrationExecutionMode,
    },
    InvalidArtifactExportConfig {
        field_path: String,
        details: String,
    },
    ArtifactExportDirectoryCreateFailed {
        path: String,
        details: String,
    },
    ArtifactExportSerializationFailed {
        artifact_kind: StylizedFactHorizonComparisonArtifactKind,
        details: String,
    },
    ArtifactExportWriteFailed {
        path: String,
        details: String,
    },
}

impl Display for StylizedFactHorizonComparisonReportGenerationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMetricCatalog { source } => {
                write!(formatter, "metric catalog is invalid: {source}")
            }
            Self::MetricCatalogIdMismatch {
                expected_catalog_id,
                found_catalog_id,
            } => write!(
                formatter,
                "comparison request metric_catalog_id `{expected_catalog_id}` does not match provided catalog_id `{found_catalog_id}`"
            ),
            Self::MetricCatalogVersionMismatch {
                expected_catalog_version,
                found_catalog_version,
            } => write!(
                formatter,
                "comparison request metric_catalog_version `{expected_catalog_version}` does not match provided catalog_version `{found_catalog_version}`"
            ),
            Self::InvalidComparisonRequest { source } => {
                write!(formatter, "comparison request is invalid: {source}")
            }
            Self::MissingMetricCatalogEntry { metric_id } => write!(
                formatter,
                "metric `{metric_id}` is not defined in the provided metric catalog"
            ),
            Self::ArtifactExportRequiresDiskMode { execution_mode } => write!(
                formatter,
                "artifact export requires execution_mode=disk, found `{execution_mode:?}`"
            ),
            Self::InvalidArtifactExportConfig {
                field_path,
                details,
            } => write!(
                formatter,
                "invalid artifact export config field `{field_path}`: {details}"
            ),
            Self::ArtifactExportDirectoryCreateFailed { path, details } => write!(
                formatter,
                "failed to create artifact export directory `{path}`: {details}"
            ),
            Self::ArtifactExportSerializationFailed {
                artifact_kind,
                details,
            } => write!(
                formatter,
                "failed to serialize artifact `{artifact_kind:?}`: {details}"
            ),
            Self::ArtifactExportWriteFailed { path, details } => {
                write!(formatter, "failed to write artifact `{path}`: {details}")
            }
        }
    }
}

impl Error for StylizedFactHorizonComparisonReportGenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMetricCatalog { source } => Some(source),
            Self::InvalidComparisonRequest { source } => Some(source),
            _ => None,
        }
    }
}

pub fn run_stylized_fact_horizon_comparison_report_generator(
    generator_request: StylizedFactHorizonComparisonReportGeneratorRequest,
) -> Result<
    StylizedFactHorizonComparisonReportBundle,
    StylizedFactHorizonComparisonReportGenerationError,
> {
    generator_request
        .metric_catalog
        .validate()
        .map_err(|source| {
            StylizedFactHorizonComparisonReportGenerationError::InvalidMetricCatalog { source }
        })?;

    if generator_request.comparison_request.metric_catalog_id
        != generator_request.metric_catalog.catalog_id
    {
        return Err(
            StylizedFactHorizonComparisonReportGenerationError::MetricCatalogIdMismatch {
                expected_catalog_id: generator_request.comparison_request.metric_catalog_id,
                found_catalog_id: generator_request.metric_catalog.catalog_id,
            },
        );
    }

    if generator_request.comparison_request.metric_catalog_version
        != generator_request.metric_catalog.catalog_version
    {
        return Err(
            StylizedFactHorizonComparisonReportGenerationError::MetricCatalogVersionMismatch {
                expected_catalog_version: generator_request
                    .comparison_request
                    .metric_catalog_version,
                found_catalog_version: generator_request.metric_catalog.catalog_version,
            },
        );
    }

    let comparison_report =
        run_stylized_fact_horizon_comparison_engine(generator_request.comparison_request.clone())
            .map_err(|source| {
            StylizedFactHorizonComparisonReportGenerationError::InvalidComparisonRequest { source }
        })?;

    let metric_class_by_metric_id =
        build_metric_class_lookup(generator_request.metric_catalog.metrics.as_slice());
    let table_rows = build_table_rows(
        comparison_report.run_reports.as_slice(),
        &metric_class_by_metric_id,
    )?;
    let table_summary = StylizedFactHorizonComparisonTableSummary {
        schema_version: STYLIZED_FACT_HORIZON_COMPARISON_TABLE_SUMMARY_SCHEMA_VERSION,
        determinism_policy: STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_DETERMINISM_POLICY,
        rows: table_rows,
    };

    let mut report_bundle = StylizedFactHorizonComparisonReportBundle {
        schema_version: STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_SCHEMA_VERSION,
        determinism_policy: STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_DETERMINISM_POLICY,
        output_schema_metadata: build_output_schema_metadata(),
        comparison_report,
        table_summary,
        artifact_export: None,
    };

    if let Some(export_config) = generator_request.generation_options.artifact_export {
        if generator_request.generation_options.execution_mode != CalibrationExecutionMode::Disk {
            return Err(
                StylizedFactHorizonComparisonReportGenerationError::ArtifactExportRequiresDiskMode {
                    execution_mode: generator_request.generation_options.execution_mode,
                },
            );
        }

        validate_artifact_export_config(&export_config)?;

        let export_receipt = build_export_receipt(&export_config);
        report_bundle.artifact_export = Some(export_receipt.clone());

        write_report_bundle_artifacts(&report_bundle, &export_receipt)?;
    }

    Ok(report_bundle)
}

fn build_metric_class_lookup(
    metric_catalog_metrics: &[super::metric_catalog::StylizedFactMetricDefinition],
) -> BTreeMap<String, StylizedFactMetricClass> {
    let mut metric_class_by_metric_id: BTreeMap<String, StylizedFactMetricClass> = BTreeMap::new();

    for metric_definition in metric_catalog_metrics {
        metric_class_by_metric_id.insert(
            metric_definition.metric_id.clone(),
            metric_definition.metric_class,
        );
    }

    metric_class_by_metric_id
}

fn build_table_rows(
    run_reports: &[StylizedFactCalibrationRunComparisonReport],
    metric_class_by_metric_id: &BTreeMap<String, StylizedFactMetricClass>,
) -> Result<
    Vec<StylizedFactHorizonComparisonTableSummaryRow>,
    StylizedFactHorizonComparisonReportGenerationError,
> {
    let mut rows: Vec<StylizedFactHorizonComparisonTableSummaryRow> = Vec::new();

    for run_report in run_reports {
        for horizon_report in &run_report.horizons {
            rows.extend(build_horizon_table_rows(
                run_report,
                horizon_report,
                metric_class_by_metric_id,
            )?);
        }
    }

    Ok(rows)
}

fn build_horizon_table_rows(
    run_report: &StylizedFactCalibrationRunComparisonReport,
    horizon_report: &StylizedFactHorizonComparison,
    metric_class_by_metric_id: &BTreeMap<String, StylizedFactMetricClass>,
) -> Result<
    Vec<StylizedFactHorizonComparisonTableSummaryRow>,
    StylizedFactHorizonComparisonReportGenerationError,
> {
    let mut rows: Vec<StylizedFactHorizonComparisonTableSummaryRow> =
        Vec::with_capacity(horizon_report.metrics.len());

    for metric_report in &horizon_report.metrics {
        rows.push(build_table_row(
            run_report,
            horizon_report,
            metric_report,
            metric_class_by_metric_id,
        )?);
    }

    Ok(rows)
}

fn build_table_row(
    run_report: &StylizedFactCalibrationRunComparisonReport,
    horizon_report: &StylizedFactHorizonComparison,
    metric_report: &StylizedFactMetricComparison,
    metric_class_by_metric_id: &BTreeMap<String, StylizedFactMetricClass>,
) -> Result<
    StylizedFactHorizonComparisonTableSummaryRow,
    StylizedFactHorizonComparisonReportGenerationError,
> {
    let metric_class = metric_class_by_metric_id
        .get(metric_report.metric_id.as_str())
        .copied()
        .ok_or_else(|| {
            StylizedFactHorizonComparisonReportGenerationError::MissingMetricCatalogEntry {
                metric_id: metric_report.metric_id.clone(),
            }
        })?;

    let empirical_sample_coverage = build_metric_coverage_summary(
        metric_report.empirical.sample_coverage,
        metric_report.empirical.observed_counts,
        metric_report.empirical.required_minimums,
    );
    let simulated_sample_coverage = build_metric_coverage_summary(
        metric_report.simulated.sample_coverage,
        metric_report.simulated.observed_counts,
        metric_report.simulated.required_minimums,
    );

    Ok(StylizedFactHorizonComparisonTableSummaryRow {
        calibration_run_id: run_report.calibration_run_id.clone(),
        empirical_dataset_id: run_report.empirical_dataset_id.clone(),
        simulated_dataset_id: run_report.simulated_dataset_id.clone(),
        horizon_label: horizon_report.horizon_label.clone(),
        metric_id: metric_report.metric_id.clone(),
        metric_name: metric_report.metric_name.clone(),
        metric_variant: metric_report.metric_variant.clone(),
        metric_class,
        alignment_status: metric_report.alignment_status,
        status: metric_report.status,
        absolute_delta: metric_report.absolute_delta,
        standardized_delta: metric_report.uncertainty_context.standardized_delta,
        empirical_status: metric_report.empirical.status,
        simulated_status: metric_report.simulated.status,
        empirical_value: metric_report.empirical.value,
        simulated_value: metric_report.simulated.value,
        empirical_confidence_interval: metric_report
            .empirical
            .confidence_interval
            .map(to_interval_bounds),
        simulated_confidence_interval: metric_report
            .simulated
            .confidence_interval
            .map(to_interval_bounds),
        empirical_sample_coverage,
        simulated_sample_coverage,
        empirical_horizon_coverage_ratio: metric_report.empirical.horizon_coverage_ratio,
        simulated_horizon_coverage_ratio: metric_report.simulated.horizon_coverage_ratio,
        insufficient_data_reasons: StylizedFactMetricInsufficientDataReasons {
            empirical: metric_report.empirical.insufficiency_reason_codes.clone(),
            simulated: metric_report.simulated.insufficiency_reason_codes.clone(),
        },
        decision_reason_codes: metric_report.decision_reason_codes.clone(),
    })
}

fn build_metric_coverage_summary(
    sample_coverage: super::comparison::StylizedFactMetricSampleCoverage,
    observed_counts: StylizedFactObservedCounts,
    required_minimums: StylizedFactSampleSufficiencyRequirements,
) -> StylizedFactMetricCoverageSummary {
    StylizedFactMetricCoverageSummary {
        horizon_interval_count: sample_coverage.horizon_interval_count,
        covered_interval_count: sample_coverage.covered_interval_count,
        sample_count: observed_counts.sample_count,
        effective_sample_count: observed_counts.effective_sample_count,
        tail_observation_count: observed_counts.tail_observation_count,
        minimum_sample_count: required_minimums.minimum_sample_count,
        minimum_effective_sample_count: required_minimums.minimum_effective_sample_count,
        minimum_tail_observation_count: required_minimums.minimum_tail_observation_count,
        minimum_horizon_coverage_ratio_scaled_bps: (required_minimums
            .minimum_horizon_coverage_ratio
            * 10_000.0)
            .round()
            .clamp(0.0, u64::MAX as f64) as u64,
    }
}

fn to_interval_bounds(
    confidence_interval: super::comparison::StylizedFactMetricConfidenceInterval,
) -> StylizedFactMetricIntervalBounds {
    StylizedFactMetricIntervalBounds {
        lower_bound: confidence_interval.lower_bound,
        upper_bound: confidence_interval.upper_bound,
        confidence_level: confidence_interval.confidence_level,
    }
}

fn build_output_schema_metadata() -> StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata {
    StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata {
        schema_version:
            STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_OUTPUT_SCHEMA_METADATA_VERSION,
        required_bundle_fields: REPORT_BUNDLE_REQUIRED_FIELDS.to_vec(),
        required_table_summary_fields: TABLE_SUMMARY_REQUIRED_FIELDS.to_vec(),
        required_table_summary_row_fields: TABLE_SUMMARY_ROW_REQUIRED_FIELDS.to_vec(),
        decision_state_vocabulary: DECISION_STATE_VOCABULARY.to_vec(),
        metric_class_vocabulary: REPORT_BUNDLE_METRIC_CLASS_VOCABULARY.to_vec(),
        insufficient_data_status_code: STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE,
    }
}

fn validate_artifact_export_config(
    export_config: &StylizedFactHorizonComparisonArtifactExportConfig,
) -> Result<(), StylizedFactHorizonComparisonReportGenerationError> {
    validate_export_non_empty_field(
        "generation_options.artifact_export.export_directory",
        &export_config.export_directory,
    )?;
    validate_export_non_empty_field(
        "generation_options.artifact_export.file_stem",
        &export_config.file_stem,
    )?;

    Ok(())
}

fn validate_export_non_empty_field(
    field_path: &'static str,
    value: &str,
) -> Result<(), StylizedFactHorizonComparisonReportGenerationError> {
    validate_non_empty_string(value, field_path).map_err(|source| {
        StylizedFactHorizonComparisonReportGenerationError::InvalidArtifactExportConfig {
            field_path: source.field.to_string(),
            details: source.message().to_string(),
        }
    })
}

fn build_export_receipt(
    export_config: &StylizedFactHorizonComparisonArtifactExportConfig,
) -> StylizedFactHorizonComparisonArtifactExportReceipt {
    let export_directory = PathBuf::from(export_config.export_directory.as_str());
    let report_file_name = format!("{}.report_bundle.json", export_config.file_stem);
    let table_file_name = format!("{}.table_summary.json", export_config.file_stem);

    let report_path = export_directory.join(report_file_name.as_str());
    let table_path = export_directory.join(table_file_name.as_str());

    StylizedFactHorizonComparisonArtifactExportReceipt {
        export_directory: export_config.export_directory.clone(),
        file_stem: export_config.file_stem.clone(),
        exported_files: vec![
            StylizedFactHorizonComparisonExportedArtifactFile {
                artifact_kind: StylizedFactHorizonComparisonArtifactKind::ReportBundle,
                file_name: report_file_name,
                absolute_path: report_path.display().to_string(),
            },
            StylizedFactHorizonComparisonExportedArtifactFile {
                artifact_kind: StylizedFactHorizonComparisonArtifactKind::TableSummary,
                file_name: table_file_name,
                absolute_path: table_path.display().to_string(),
            },
        ],
    }
}

fn write_report_bundle_artifacts(
    report_bundle: &StylizedFactHorizonComparisonReportBundle,
    export_receipt: &StylizedFactHorizonComparisonArtifactExportReceipt,
) -> Result<(), StylizedFactHorizonComparisonReportGenerationError> {
    let export_directory = PathBuf::from(export_receipt.export_directory.as_str());
    fs::create_dir_all(export_directory.as_path()).map_err(|source| {
        StylizedFactHorizonComparisonReportGenerationError::ArtifactExportDirectoryCreateFailed {
            path: export_directory.display().to_string(),
            details: source.to_string(),
        }
    })?;

    let report_path = exported_path(
        export_receipt,
        StylizedFactHorizonComparisonArtifactKind::ReportBundle,
    );
    let table_path = exported_path(
        export_receipt,
        StylizedFactHorizonComparisonArtifactKind::TableSummary,
    );

    write_report_json_artifact(
        report_path.as_path(),
        report_bundle,
        StylizedFactHorizonComparisonArtifactKind::ReportBundle,
    )?;
    write_report_json_artifact(
        table_path.as_path(),
        &report_bundle.table_summary,
        StylizedFactHorizonComparisonArtifactKind::TableSummary,
    )?;

    Ok(())
}

fn write_report_json_artifact(
    path: &Path,
    payload: &impl Serialize,
    artifact_kind: StylizedFactHorizonComparisonArtifactKind,
) -> Result<(), StylizedFactHorizonComparisonReportGenerationError> {
    write_pretty_json_document(path, payload)
        .map_err(|source| report_json_write_error(source, artifact_kind))
}

fn report_json_write_error(
    source: JsonDocumentWriteError,
    artifact_kind: StylizedFactHorizonComparisonArtifactKind,
) -> StylizedFactHorizonComparisonReportGenerationError {
    match source {
        JsonDocumentWriteError::Serialize { source } => {
            StylizedFactHorizonComparisonReportGenerationError::ArtifactExportSerializationFailed {
                artifact_kind,
                details: source.to_string(),
            }
        }
        JsonDocumentWriteError::CreateDirectory { path, source }
        | JsonDocumentWriteError::Create { path, source }
        | JsonDocumentWriteError::Write { path, source } => {
            StylizedFactHorizonComparisonReportGenerationError::ArtifactExportWriteFailed {
                path: path.display().to_string(),
                details: source.to_string(),
            }
        }
    }
}

fn exported_path(
    export_receipt: &StylizedFactHorizonComparisonArtifactExportReceipt,
    artifact_kind: StylizedFactHorizonComparisonArtifactKind,
) -> PathBuf {
    for exported_file in &export_receipt.exported_files {
        if exported_file.artifact_kind == artifact_kind {
            return PathBuf::from(exported_file.absolute_path.as_str());
        }
    }

    PathBuf::from(export_receipt.export_directory.as_str())
}

const DECISION_STATE_VOCABULARY: [&str; 3] = ["pass", "fail", "insufficient_data"];
