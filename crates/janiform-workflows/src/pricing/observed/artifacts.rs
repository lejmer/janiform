use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use janiform_terminal_ui::{CompletionSummary, CompletionSummaryStatus, KeyValueRow, StartSummary};
use serde_json::json;

use crate::artifact_io::{ArtifactWriteKind, write_pretty_json_file};
use crate::error::WorkflowError;

use super::super::artifacts::{PricingArtifactPaths, pricing_artifact_paths};
use super::physical_distribution::ObservedOptionsPhysicalDistributionDiagnosticsOutput;
use super::types::{
    ObservedOptionsPricingNode, ObservedOptionsPricingOutput, ObservedOptionsPricingRequest,
    ObservedOptionsPricingScope, ObservedOptionsSnapshotResult,
};

const PHYSICAL_DISTRIBUTION_NODES_FILE_NAME: &str = "physical_distribution_nodes.json";
const PHYSICAL_DISTRIBUTION_REPORT_FILE_NAME: &str = "physical_distribution_report.json";

pub(super) fn write_observed_artifacts_if_requested(
    output_path: Option<&PathBuf>,
    run_id: &str,
    scope: ObservedOptionsPricingScope,
    valuation_time_utc: DateTime<Utc>,
    nodes: &[ObservedOptionsPricingNode],
    snapshot_results: &[ObservedOptionsSnapshotResult],
    physical_distribution: Option<&ObservedOptionsPhysicalDistributionDiagnosticsOutput>,
    execution_duration: Duration,
) -> Result<Option<PricingArtifactPaths>, WorkflowError> {
    let Some(output_path) = output_path else {
        return Ok(None);
    };

    let mut paths = pricing_artifact_paths(output_path);
    if physical_distribution.is_some() {
        paths.physical_distribution_nodes_path = Some(
            paths
                .run_directory
                .join(PHYSICAL_DISTRIBUTION_NODES_FILE_NAME),
        );
        paths.physical_distribution_report_path = Some(
            paths
                .run_directory
                .join(PHYSICAL_DISTRIBUTION_REPORT_FILE_NAME),
        );
    }

    let primary_payload = json!({
        "schema_version": "janiform.observed_options_pricing_result.v1",
        "run_id": run_id,
        "scope": scope,
        "valuation_time_utc": valuation_time_utc.to_rfc3339(),
        "nodes": nodes,
        "snapshot_results": snapshot_results,
        "physical_distribution": physical_distribution,
        "artifact_paths": null,
        "execution_duration_seconds": execution_duration.as_secs_f64(),
    });

    write_pretty_json_file(
        &paths.primary_output_path,
        &primary_payload,
        ArtifactWriteKind::Pricing,
    )?;

    if let Some(physical_distribution) = physical_distribution {
        if let Some(path) = paths.physical_distribution_nodes_path.as_ref() {
            write_pretty_json_file(
                path,
                &json!({
                    "schema_version": physical_distribution.schema_version,
                    "request": physical_distribution.request,
                    "nodes": &physical_distribution.nodes,
                }),
                ArtifactWriteKind::Pricing,
            )?;
        }

        if let Some(path) = paths.physical_distribution_report_path.as_ref() {
            write_pretty_json_file(
                path,
                &physical_distribution.report,
                ArtifactWriteKind::Pricing,
            )?;
        }
    }

    write_pretty_json_file(
        &paths.report_path,
        &json!({
            "schema_version": "janiform.workflow_report.v1",
            "workflow": "price observed-options",
            "primary_output_path": paths.primary_output_path,
            "snapshot_count": snapshot_results.len(),
            "node_count": snapshot_results.iter().map(|result| result.nodes.len()).sum::<usize>(),
            "physical_distribution_nodes_path": &paths.physical_distribution_nodes_path,
            "physical_distribution_report_path": &paths.physical_distribution_report_path,
            "physical_distribution": physical_distribution.map(|diagnostics| &diagnostics.report),
            "execution_duration_seconds": execution_duration.as_secs_f64(),
        }),
        ArtifactWriteKind::Pricing,
    )?;

    write_pretty_json_file(
        &paths.manifest_path,
        &json!({
            "schema_version": "janiform.run_manifest.v1",
            "workflow": "price observed-options",
            "run_id": run_id,
            "run_directory": paths.run_directory,
            "primary_output_path": paths.primary_output_path,
            "report_path": paths.report_path,
            "physical_distribution_nodes_path": &paths.physical_distribution_nodes_path,
            "physical_distribution_report_path": &paths.physical_distribution_report_path,
        }),
        ArtifactWriteKind::Pricing,
    )?;

    Ok(Some(paths))
}

pub(super) fn observed_options_start_summary(
    run_id: &str,
    request: &ObservedOptionsPricingRequest,
) -> StartSummary {
    StartSummary {
        title: "Janiform price".to_string(),
        rows: vec![
            KeyValueRow::new("Run", run_id),
            KeyValueRow::new("Scope", "observed options"),
            KeyValueRow::new("Ticks", request.tick_data_source.path.as_str()),
            KeyValueRow::new("Options", request.option_surface_source.path.as_str()),
        ],
    }
}

pub(super) fn observed_options_completion_summary(
    output: &ObservedOptionsPricingOutput,
    elapsed: Duration,
) -> CompletionSummary {
    let priced_node_count = output
        .nodes
        .iter()
        .filter(|node| node.model_price.is_some())
        .count();

    CompletionSummary {
        status: CompletionSummaryStatus::Succeeded,
        title: format!(
            "Observed option pricing completed in {}",
            janiform_terminal_ui::format_elapsed_label(elapsed)
        ),
        rows: vec![
            KeyValueRow::new("Snapshots", output.snapshot_results.len().to_string()),
            KeyValueRow::new("Priced nodes", priced_node_count.to_string()),
            KeyValueRow::new(
                "Failed nodes",
                output
                    .nodes
                    .len()
                    .saturating_sub(priced_node_count)
                    .to_string(),
            ),
        ],
    }
}

pub(super) fn observed_pricing_run_id(scope: &str) -> String {
    format!("{scope}_price_{}", Utc::now().format("%Y%m%dT%H%M%SZ"))
}
