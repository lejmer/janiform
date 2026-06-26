use std::path::Path;
use std::time::Duration;

use janiform_foundations::paths::format_path;
use janiform_terminal_ui::{
    CompletionSummary, CompletionSummaryStatus, KeyValueRow, StartSummary, format_count,
    format_elapsed_label,
};
use serde_json::{Value, json};

use janiform::pricing::{
    VanillaOptionGridBatchPricingExecutionInput, VanillaOptionGridBatchPricingRunResult,
    VanillaOptionGridPricingResult, VanillaOptionPricingRunResult,
    VanillaOptionSurfacePricingExecutionInput, VanillaOptionSurfacePricingRunResult,
};

use crate::error::WorkflowError;

use super::artifacts::{PricingArtifactPaths, formatted_pricing_artifact_paths};
use super::labels::{
    fitted_model_pricing_workflow_label, fitted_model_single_option_pricing_scope_label,
    vanilla_option_grid_batch_pricing_scope_label, vanilla_option_surface_pricing_scope_label,
};

/// Completed fitted-model single-option pricing workflow.
pub struct FittedModelSingleOptionPricingOutput {
    /// Stable pricing run id.
    pub run_id: String,
    /// Native pricing result.
    pub run_result: VanillaOptionPricingRunResult,
    /// Optional artifact paths.
    pub artifact_paths: Option<PricingArtifactPaths>,
    /// Native workflow duration.
    pub execution_duration: Duration,
}

impl FittedModelSingleOptionPricingOutput {
    /// Serialize the completion payload for machine output.
    pub fn completion_payload(&self, model_path: &Path) -> Result<Value, WorkflowError> {
        let response = serde_json::to_value(&self.run_result.response)
            .map_err(|source| WorkflowError::OutputSerialize { source })?;

        let run_diagnostics = serde_json::to_value(&self.run_result.run_diagnostics)
            .map_err(|source| WorkflowError::OutputSerialize { source })?;

        let artifact_paths = self
            .artifact_paths
            .as_ref()
            .map(formatted_pricing_artifact_paths);

        Ok(json!({
            "schema_version": "janiform.price_run.v1",
            "command_id": fitted_model_pricing_workflow_label(),
            "status": "completed",
            "run_id": self.run_id,
            "model_path": format_path(model_path),
            "primary_output_path": artifact_paths.as_ref().map(|paths| paths.primary_output_path.as_str()),
            "report_path": artifact_paths.as_ref().map(|paths| paths.report_path.as_str()),
            "manifest_path": artifact_paths.as_ref().map(|paths| paths.manifest_path.as_str()),
            "run_directory": artifact_paths.as_ref().map(|paths| paths.run_directory.as_str()),
            "priced_node_count": 1,
            "failed_node_count": single_failed_node_count(&self.run_result),
            "execution_duration_seconds": self.execution_duration.as_secs_f64(),
            "response": response,
            "run_diagnostics": run_diagnostics,
        }))
    }
}

pub(super) fn pricing_start_summary(
    run_id: &str,
    model_path: Option<&Path>,
    output_path: Option<&Path>,
    valuation_time_utc: &str,
) -> StartSummary {
    let input = match model_path {
        Some(model_path) => format!("model: {}", format_path(model_path)),
        None => "model: provided payload".to_string(),
    };

    let mut rows = vec![
        KeyValueRow::new("Run", run_id),
        KeyValueRow::new("Scope", fitted_model_single_option_pricing_scope_label()),
        KeyValueRow::block("Inputs", input),
        KeyValueRow::new("Valuation", valuation_time_utc),
    ];

    if let Some(output_path) = output_path {
        rows.push(KeyValueRow::new("Output", format_path(output_path)));
    }

    StartSummary {
        title: "Janiform price".to_string(),
        rows,
    }
}

pub(super) fn pricing_completion_summary(
    output: &FittedModelSingleOptionPricingOutput,
    elapsed: Duration,
) -> CompletionSummary {
    let failed_node_count = if output.run_result.response.price.is_some() {
        0
    } else {
        1
    };

    let mut rows = vec![
        KeyValueRow::new("Priced nodes", "1"),
        KeyValueRow::new("Failed nodes", failed_node_count.to_string()),
    ];

    if let Some(paths) = &output.artifact_paths {
        rows.push(KeyValueRow::new(
            "Path",
            format_path(&paths.primary_output_path),
        ));

        rows.push(KeyValueRow::new("Report", format_path(&paths.report_path)));
    }

    CompletionSummary {
        status: CompletionSummaryStatus::Succeeded,
        title: format!("Pricing completed in {}", format_elapsed_label(elapsed)),
        rows,
    }
}

pub(super) fn grid_batch_pricing_start_summary(
    input: &VanillaOptionGridBatchPricingExecutionInput,
) -> StartSummary {
    let node_count = grid_batch_requested_node_count(input);

    StartSummary {
        title: "Janiform price".to_string(),
        rows: vec![
            KeyValueRow::new("Run", "pricing"),
            KeyValueRow::new("Scope", vanilla_option_grid_batch_pricing_scope_label()),
            KeyValueRow::new(
                "Grids",
                format_count(input.request.grids.len(), "grid", "grids"),
            ),
            KeyValueRow::new("Requested nodes", format_count(node_count, "node", "nodes")),
            KeyValueRow::new(
                "Runtime intervals",
                format_count(
                    input.runtime_interval_schedule.interval_count(),
                    "interval",
                    "intervals",
                ),
            ),
        ],
    }
}

pub(super) fn grid_batch_pricing_completion_summary(
    result: &VanillaOptionGridBatchPricingRunResult,
    elapsed: Duration,
) -> CompletionSummary {
    let priced_node_count = result
        .grids
        .iter()
        .map(priced_grid_node_count)
        .sum::<usize>();
    let node_count = result.grids.iter().map(grid_node_count).sum::<usize>();

    pricing_grid_completion_summary(
        "Pricing grid completed",
        priced_node_count,
        node_count,
        elapsed,
    )
}

pub(super) fn surface_pricing_start_summary(
    input: &VanillaOptionSurfacePricingExecutionInput,
) -> StartSummary {
    let node_count = input.request.market_inputs.strikes.len()
        * input.request.market_inputs.maturity_intervals.len();

    StartSummary {
        title: "Janiform price".to_string(),
        rows: vec![
            KeyValueRow::new("Run", "pricing"),
            KeyValueRow::new("Scope", vanilla_option_surface_pricing_scope_label()),
            KeyValueRow::new("Requested nodes", format_count(node_count, "node", "nodes")),
            KeyValueRow::new(
                "Runtime intervals",
                format_count(
                    input.runtime_interval_schedule.interval_count(),
                    "interval",
                    "intervals",
                ),
            ),
        ],
    }
}

pub(super) fn surface_pricing_completion_summary(
    result: &VanillaOptionSurfacePricingRunResult,
    elapsed: Duration,
) -> CompletionSummary {
    let priced_node_count = result
        .response
        .price_grid
        .iter()
        .flatten()
        .filter(|price| price.is_some())
        .count();
    let node_count = result
        .response
        .price_grid
        .iter()
        .map(Vec::len)
        .sum::<usize>();

    pricing_grid_completion_summary(
        "Pricing surface completed",
        priced_node_count,
        node_count,
        elapsed,
    )
}

fn grid_batch_requested_node_count(input: &VanillaOptionGridBatchPricingExecutionInput) -> usize {
    input
        .request
        .grids
        .iter()
        .map(|grid| grid.strikes.len() * grid.maturity_intervals.len())
        .sum()
}

fn grid_node_count(grid: &VanillaOptionGridPricingResult) -> usize {
    grid.price_grid.iter().map(Vec::len).sum()
}

fn priced_grid_node_count(grid: &VanillaOptionGridPricingResult) -> usize {
    grid.price_grid
        .iter()
        .flatten()
        .filter(|price| price.is_some())
        .count()
}

fn pricing_grid_completion_summary(
    title_prefix: &str,
    priced_node_count: usize,
    node_count: usize,
    elapsed: Duration,
) -> CompletionSummary {
    CompletionSummary {
        status: CompletionSummaryStatus::Succeeded,
        title: format!("{title_prefix} in {}", format_elapsed_label(elapsed)),
        rows: vec![
            KeyValueRow::new(
                "Priced nodes",
                format_count(priced_node_count, "node", "nodes"),
            ),
            KeyValueRow::new(
                "Failed nodes",
                format_count(node_count - priced_node_count, "node", "nodes"),
            ),
        ],
    }
}

fn single_failed_node_count(run_result: &VanillaOptionPricingRunResult) -> usize {
    if run_result.response.price.is_some() {
        0
    } else {
        1
    }
}
