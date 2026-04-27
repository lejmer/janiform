use std::path::Path;
use std::time::Duration;

use janiform_terminal_ui::{
    CompletionSummary, CompletionSummaryStatus, KeyValueRow, StartSummary, format_artifact_path,
    format_elapsed_label, format_path,
};
use janiform_workflows::simulation::SimulationExportSummary;

pub(crate) fn simulation_start_summary(
    run_id: &str,
    simulation_kind: &str,
    request_path: &Path,
    output_path: Option<&Path>,
) -> StartSummary {
    let mut rows = vec![
        KeyValueRow::new("Run", run_id),
        KeyValueRow::new("Scope", simulation_kind),
        KeyValueRow::block("Inputs", format!("request: {}", format_path(request_path))),
    ];

    if let Some(output_path) = output_path {
        rows.push(KeyValueRow::new("Output", format_path(output_path)));
    }

    StartSummary {
        title: "Janiform simulate".to_string(),
        rows,
    }
}

pub(crate) fn simulation_completion_summary(
    simulation_kind: &str,
    seed: u64,
    output_path: Option<&Path>,
    export_summary: Option<&SimulationExportSummary>,
    elapsed: Duration,
    simulated_interval_count: usize,
) -> CompletionSummary {
    let mut rows = vec![
        KeyValueRow::new("Kind", simulation_kind),
        KeyValueRow::new("Seed", seed.to_string()),
        KeyValueRow::new("Intervals", simulated_interval_count.to_string()),
    ];

    if let Some(output_path) = output_path {
        rows.push(KeyValueRow::new("Path", format_artifact_path(output_path)));
    }

    if let Some(export_summary) = export_summary {
        rows.push(KeyValueRow::new(
            "Records",
            export_summary.record_count.to_string(),
        ));
        rows.push(KeyValueRow::new(
            "Chunks",
            export_summary.chunk_count.to_string(),
        ));
    }

    CompletionSummary {
        status: CompletionSummaryStatus::Succeeded,
        title: format!("Simulation completed in {}", format_elapsed_label(elapsed)),
        rows,
    }
}
