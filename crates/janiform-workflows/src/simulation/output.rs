use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::WorkflowError;

use janiform_terminal_ui::{
    CompletionSummary, CompletionSummaryStatus, KeyValueRow, StartSummary, format_artifact_path,
    format_elapsed_label, format_path,
};

use super::intervals::simulation_interval_count_label;
use super::{DirectSimulationCompleted, DirectSimulationCompletion, SimulationExportSummary};

/// Output from a simulation workflow.
pub struct SimulationWorkflowOutput {
    /// Completed native simulation result.
    pub completed: DirectSimulationCompleted,
    /// Optional export summary.
    pub export_summary: Option<SimulationExportSummary>,
    /// Native workflow execution duration.
    pub execution_duration: Duration,
}

impl SimulationWorkflowOutput {
    /// Serialize the public workflow metadata fields.
    pub fn response_payload(&self) -> Result<Value, WorkflowError> {
        let completion = self.completed.completion();

        let simulation_kind = match &self.completed {
            DirectSimulationCompleted::SeededTick { .. } => "seeded_tick",
            DirectSimulationCompleted::CoarseInterval { .. } => "coarse_interval",
        };

        let export_summary = self.export_summary.as_ref().map(|summary| {
            json!({
                "record_count": summary.record_count,
                "chunk_count": summary.chunk_count,
            })
        });

        Ok(json!({
            "simulation_kind": simulation_kind,
            "completion": {
                "seed": completion.seed,
                "simulated_interval_count": completion.simulated_interval_count,
            },
            "export_summary": export_summary,
            "execution_duration_seconds": self.execution_duration.as_secs_f64(),
        }))
    }
}

pub(super) fn simulation_start_summary(
    run_id: &str,
    simulation_kind: &str,
    request_path: Option<&Path>,
    output_path: Option<&Path>,
) -> StartSummary {
    let input = match request_path {
        Some(request_path) => format!("request: {}", format_path(request_path)),
        None => "request: provided payload".to_string(),
    };

    let mut rows = vec![
        KeyValueRow::new("Run", run_id),
        KeyValueRow::new("Scope", simulation_kind),
        KeyValueRow::block("Inputs", input),
    ];

    if let Some(output_path) = output_path {
        rows.push(KeyValueRow::new("Output", format_path(output_path)));
    }

    StartSummary {
        title: "Janiform simulate".to_string(),
        rows,
    }
}

pub(super) fn simulation_completion_summary(
    simulation_kind: &str,
    completion: DirectSimulationCompletion,
    output_path: Option<&Path>,
    export_summary: Option<&SimulationExportSummary>,
    elapsed: Duration,
) -> CompletionSummary {
    let mut rows = vec![
        KeyValueRow::new("Kind", simulation_kind),
        KeyValueRow::new("Seed", completion.seed.to_string()),
        KeyValueRow::new(
            "Generated",
            simulation_interval_count_label(
                completion.simulated_interval_count,
                completion.simulated_interval_duration_nanos,
            ),
        ),
    ];

    if let Some(generated_tick_count) = completion.generated_tick_count {
        rows.push(KeyValueRow::new("Ticks", generated_tick_count.to_string()));
    }

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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use janiform_foundations::time::NANOS_PER_HOUR;

    use super::DirectSimulationCompletion;
    use super::simulation_completion_summary;

    #[test]
    fn completion_summary_labels_intervals_with_duration_width_and_seeded_ticks() {
        let summary = simulation_completion_summary(
            "Seeded tick simulation",
            DirectSimulationCompletion {
                seed: 17,
                simulated_interval_count: 168,
                simulated_interval_duration_nanos: Some(NANOS_PER_HOUR),
                generated_tick_count: Some(654_321),
            },
            None,
            None,
            Duration::from_secs(1),
        );

        assert_eq!(summary_row_value(&summary, "Generated"), "168 1h intervals");
        assert_eq!(summary_row_value(&summary, "Ticks"), "654321");
    }

    fn summary_row_value<'summary>(
        summary: &'summary janiform_terminal_ui::CompletionSummary,
        label: &str,
    ) -> &'summary str {
        summary
            .rows
            .iter()
            .find(|row| row.label == label)
            .map(|row| row.value.as_str())
            .expect("summary row should exist")
    }
}
