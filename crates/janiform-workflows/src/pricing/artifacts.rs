use std::path::{Path, PathBuf};

use janiform::pricing::VanillaOptionPricingRunResult;
use serde::Serialize;
use serde_json::json;

use crate::artifact_io::{ArtifactWriteKind, write_pretty_json_file};
use crate::error::WorkflowError;

use super::labels::fitted_model_pricing_workflow_label;

/// Pricing artifact paths.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PricingArtifactPaths {
    /// Run directory.
    pub run_directory: PathBuf,
    /// Primary pricing output path.
    pub primary_output_path: PathBuf,
    /// Report path.
    pub report_path: PathBuf,
    /// Run manifest path.
    pub manifest_path: PathBuf,
    /// Optional observed-options physical distribution diagnostic nodes path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_distribution_nodes_path: Option<PathBuf>,
    /// Optional observed-options physical distribution diagnostic report path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_distribution_report_path: Option<PathBuf>,
}

pub(super) fn pricing_artifact_paths(output_path: &Path) -> PricingArtifactPaths {
    let primary_output_path = if output_path.extension().is_none() {
        output_path.with_extension("json")
    } else {
        output_path.to_path_buf()
    };

    let run_directory = primary_output_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    PricingArtifactPaths {
        report_path: run_directory.join("pricing_report.json"),
        manifest_path: run_directory.join("run.json"),
        run_directory,
        primary_output_path,
        physical_distribution_nodes_path: None,
        physical_distribution_report_path: None,
    }
}

pub(super) fn write_pricing_artifacts(
    paths: &PricingArtifactPaths,
    run_id: &str,
    model_run_id: &str,
    run_result: &VanillaOptionPricingRunResult,
) -> Result<(), WorkflowError> {
    write_pretty_json_file(
        &paths.primary_output_path,
        run_result,
        ArtifactWriteKind::Pricing,
    )?;

    write_pretty_json_file(
        &paths.report_path,
        &json!({
            "schema_version": "janiform.workflow_report.v1",
            "workflow": fitted_model_pricing_workflow_label(),
            "primary_output_path": paths.primary_output_path,
            "response": run_result.response,
            "run_diagnostics": run_result.run_diagnostics,
        }),
        ArtifactWriteKind::Pricing,
    )?;

    write_pretty_json_file(
        &paths.manifest_path,
        &json!({
            "schema_version": "janiform.run_manifest.v1",
            "workflow": fitted_model_pricing_workflow_label(),
            "run_id": run_id,
            "run_directory": paths.run_directory,
            "primary_output_path": paths.primary_output_path,
            "report_path": paths.report_path,
            "artifacts": {
                "model_run_id": model_run_id,
            },
        }),
        ArtifactWriteKind::Pricing,
    )
}
