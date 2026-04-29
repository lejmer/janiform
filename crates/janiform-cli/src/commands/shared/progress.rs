use std::path::Path;

use chrono::Utc;
use janiform_terminal_ui::{ProgressAdapter, ProgressEvent, ProgressMeasure};

use crate::commands::shared::terminal::terminal_progress_adapter_for_context;
use crate::context::CommandContext;
use crate::error::CliError;

pub(crate) struct TerminalCommandProgress {
    adapter: Option<ProgressAdapter>,
    total_units: usize,
}

impl TerminalCommandProgress {
    pub(crate) fn start(
        context: &CommandContext,
        scope: &str,
        input_count: usize,
        output_path: Option<&Path>,
        phase_sequence: &[&str],
    ) -> Result<Self, CliError> {
        let mut adapter = terminal_progress_adapter_for_context(context);

        if let Some(adapter) = adapter.as_mut() {
            adapter.observe(ProgressEvent::RunStarted {
                run_id: format!(
                    "{}_{}",
                    scope.replace(' ', "_"),
                    Utc::now().format("%Y%m%dT%H%M%SZ")
                ),
                scope: scope.to_string(),
                input_count,
                output_path: output_path.map(janiform_terminal_ui::format_path),
                stage_sequence: phase_sequence
                    .iter()
                    .map(|phase| (*phase).to_string())
                    .collect(),
            });

            adapter
                .draw_latest()
                .map_err(|source| CliError::OutputProgress { source })?;
        }

        Ok(Self {
            adapter,
            total_units: phase_sequence.len(),
        })
    }

    pub(crate) fn observe(&mut self, phase: &str, completed_units: usize) -> Result<(), CliError> {
        let Some(adapter) = self.adapter.as_mut() else {
            return Ok(());
        };

        adapter.observe(ProgressEvent::RunProgress {
            phase: phase.to_string(),
            measure: ProgressMeasure::bounded(completed_units, self.total_units)
                .map_err(|source| CliError::OutputProgress { source })?,
            latest_signal: Some(phase.to_string()),
        });

        adapter
            .draw_latest()
            .map_err(|source| CliError::OutputProgress { source })
    }

    pub(crate) fn complete(&mut self) -> Result<(), CliError> {
        let Some(adapter) = self.adapter.as_mut() else {
            return Ok(());
        };

        adapter.observe(ProgressEvent::RunCompleted {
            status: "completed".to_string(),
        });

        adapter
            .finish_and_keep()
            .map_err(|source| CliError::OutputWrite { source })
    }
}
