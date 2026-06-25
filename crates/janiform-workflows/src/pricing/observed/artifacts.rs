use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use janiform::calibration::market_data::ResolvedPriceCoordinateSet;
use janiform_foundations::paths::format_path;
use janiform_terminal_ui::{CompletionSummary, CompletionSummaryStatus, KeyValueRow, StartSummary};
use serde_json::json;

use crate::artifact_io::{ArtifactWriteKind, write_pretty_json_file};
use crate::error::WorkflowError;
use crate::price_coordinates::price_coordinate_resolution_summary;

use super::super::artifacts::{PricingArtifactPaths, pricing_artifact_paths};
use super::super::labels::{
    observed_options_pricing_scope_label, observed_options_pricing_scope_token,
    observed_options_pricing_workflow_label,
};
use super::types::{
    ObservedOptionsPricingNode, ObservedOptionsPricingOutput, ObservedOptionsPricingRequest,
    ObservedOptionsPricingScope, ObservedOptionsSnapshotResult,
};

pub(super) fn write_observed_artifacts_if_requested(
    input: ObservedArtifactWriteInput<'_>,
) -> Result<Option<PricingArtifactPaths>, WorkflowError> {
    let ObservedArtifactWriteInput {
        output_path,
        run_id,
        scope,
        valuation_time_utc,
        nodes,
        snapshot_results,
        price_coordinate_resolution,
        warnings,
        execution_duration,
    } = input;

    let Some(output_path) = output_path else {
        return Ok(None);
    };

    let paths = pricing_artifact_paths(output_path);

    let primary_payload = json!({
        "schema_version": "janiform.observed_options_pricing_result.v1",
        "run_id": run_id,
        "scope": observed_options_pricing_scope_token(scope),
        "valuation_time_utc": valuation_time_utc.to_rfc3339(),
        "nodes": nodes,
        "snapshot_results": snapshot_results,
        "price_coordinate_resolution": price_coordinate_resolution,
        "warnings": warnings,
        "artifact_paths": null,
        "execution_duration_seconds": execution_duration.as_secs_f64(),
    });

    write_pretty_json_file(
        &paths.primary_output_path,
        &primary_payload,
        ArtifactWriteKind::Pricing,
    )?;

    write_pretty_json_file(
        &paths.report_path,
        &json!({
            "schema_version": "janiform.workflow_report.v1",
            "workflow": observed_options_pricing_workflow_label(),
            "primary_output_path": paths.primary_output_path,
            "snapshot_count": snapshot_results.len(),
            "node_count": snapshot_results.iter().map(|result| result.nodes.len()).sum::<usize>(),
            "price_coordinate_resolution": price_coordinate_resolution,
            "warnings": warnings,
            "execution_duration_seconds": execution_duration.as_secs_f64(),
        }),
        ArtifactWriteKind::Pricing,
    )?;

    write_pretty_json_file(
        &paths.manifest_path,
        &json!({
            "schema_version": "janiform.run_manifest.v1",
            "workflow": observed_options_pricing_workflow_label(),
            "run_id": run_id,
            "run_directory": paths.run_directory,
            "primary_output_path": paths.primary_output_path,
            "report_path": paths.report_path,
        }),
        ArtifactWriteKind::Pricing,
    )?;

    Ok(Some(paths))
}

pub(super) struct ObservedArtifactWriteInput<'a> {
    pub(super) output_path: Option<&'a PathBuf>,
    pub(super) run_id: &'a str,
    pub(super) scope: ObservedOptionsPricingScope,
    pub(super) valuation_time_utc: DateTime<Utc>,
    pub(super) nodes: &'a [ObservedOptionsPricingNode],
    pub(super) snapshot_results: &'a [ObservedOptionsSnapshotResult],
    pub(super) price_coordinate_resolution: Option<&'a ResolvedPriceCoordinateSet>,
    pub(super) warnings: &'a [String],
    pub(super) execution_duration: Duration,
}

pub(super) fn observed_options_start_summary(
    run_id: &str,
    request: &ObservedOptionsPricingRequest,
) -> StartSummary {
    StartSummary {
        title: "Janiform price".to_string(),
        rows: vec![
            KeyValueRow::new("Run", run_id),
            KeyValueRow::new("Scope", observed_options_pricing_scope_label()),
            KeyValueRow::new(
                "Ticks",
                format_path(Path::new(request.tick_data_source.path.as_str())),
            ),
            KeyValueRow::new(
                "Options",
                format_path(Path::new(request.option_surface_source.path.as_str())),
            ),
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

    let mut rows = vec![
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
    ];

    if let Some(price_coordinate_summary) = observed_price_coordinate_summary(output) {
        rows.push(KeyValueRow::block(
            "Price coordinates",
            price_coordinate_summary,
        ));
    }

    if !output.warnings.is_empty() {
        rows.push(KeyValueRow::block("Warnings", output.warnings.join("\n")));
    }

    CompletionSummary {
        status: if output.warnings.is_empty() {
            CompletionSummaryStatus::Succeeded
        } else {
            CompletionSummaryStatus::SucceededWithWarnings
        },
        title: format!(
            "Observed option pricing completed in {}",
            janiform_terminal_ui::format_elapsed_label(elapsed)
        ),
        rows,
    }
}

pub(super) fn observed_pricing_run_id(scope: &str) -> String {
    format!("{scope}_price_{}", Utc::now().format("%Y%m%dT%H%M%SZ"))
}

fn observed_price_coordinate_summary(output: &ObservedOptionsPricingOutput) -> Option<String> {
    let resolution = serde_json::to_value(&output.price_coordinate_resolution).ok()?;

    price_coordinate_resolution_summary(&resolution)
}
