use std::path::Path;
use std::time::Instant;

use chrono::Utc;
use janiform_terminal_ui::{
    ProgressAdapter, ProgressEvent, ProgressMeasure, ProgressWorkItemColumn, ProgressWorkItemMetric,
};

use crate::commands::shared::terminal::terminal_progress_adapter_for_context;
use crate::context::CommandContext;
use crate::error::CliError;

pub(crate) struct TerminalCommandProgress {
    adapter: Option<ProgressAdapter>,
    total_units: usize,
    active_stage: Option<ActiveProgressStage>,
    completed_stage_count: usize,
}

struct ActiveProgressStage {
    name: String,
    number: usize,
    started_at: Instant,
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
                completed_stage_count: 0,
            });

            adapter
                .draw_latest()
                .map_err(|source| CliError::OutputProgress {
                    source: Box::new(source),
                })?;
        }

        Ok(Self {
            adapter,
            total_units: phase_sequence.len(),
            active_stage: None,
            completed_stage_count: 0,
        })
    }

    #[cfg(test)]
    pub(crate) fn observe(&mut self, phase: &str, completed_units: usize) -> Result<(), CliError> {
        self.observe_stage_progress(StageProgressUpdate {
            phase,
            stage_number: completed_units,
            completed_units: None,
            total_units: None,
            work_item_label: None,
            work_item_progress_percent: None,
            planned_work_item_labels: Vec::new(),
            work_item_columns: Vec::new(),
            work_item_metrics: Vec::new(),
            latest_signal: None,
        })
    }

    pub(crate) fn observe_stage_progress(
        &mut self,
        update: StageProgressUpdate<'_>,
    ) -> Result<(), CliError> {
        if self.adapter.is_none() {
            return Ok(());
        }

        let stage_number = update.stage_number;

        ProgressMeasure::bounded(stage_number, self.total_units).map_err(|source| {
            CliError::OutputProgress {
                source: Box::new(source),
            }
        })?;

        if self
            .active_stage
            .as_ref()
            .is_some_and(|stage| stage.number != stage_number)
        {
            self.complete_active_stage()?;
        }

        if self.active_stage.is_none() {
            self.start_stage(update.phase, stage_number);
        }

        let measure = match (update.completed_units, update.total_units) {
            (Some(completed_units), Some(total_units)) => {
                ProgressMeasure::bounded(completed_units, total_units).map_err(|source| {
                    CliError::OutputProgress {
                        source: Box::new(source),
                    }
                })?
            }
            _ => ProgressMeasure::bounded(self.completed_stage_count, self.total_units).map_err(
                |source| CliError::OutputProgress {
                    source: Box::new(source),
                },
            )?,
        };

        let Some(adapter) = self.adapter.as_mut() else {
            return Ok(());
        };

        adapter.observe(ProgressEvent::StageProgress {
            phase: update.phase.to_string(),
            measure,
            work_item_label: update.work_item_label,
            work_item_progress_percent: update.work_item_progress_percent,
            work_item_status: None,
            planned_work_item_labels: update.planned_work_item_labels,
            work_item_columns: update.work_item_columns,
            work_item_metrics: update.work_item_metrics,
            latest_signal: Some(
                update
                    .latest_signal
                    .unwrap_or_else(|| update.phase.to_string()),
            ),
        });

        adapter
            .draw_latest()
            .map_err(|source| CliError::OutputProgress {
                source: Box::new(source),
            })
    }

    pub(crate) fn complete(&mut self) -> Result<(), CliError> {
        if self.adapter.is_none() {
            return Ok(());
        }

        if self.active_stage.is_some() {
            self.complete_active_stage()?;
        }

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

    fn start_stage(&mut self, phase: &str, stage_number: usize) {
        let Some(adapter) = self.adapter.as_mut() else {
            return;
        };

        adapter.observe(ProgressEvent::StageStarted {
            stage: phase.to_string(),
            stage_number,
            stage_count: self.total_units,
        });

        self.active_stage = Some(ActiveProgressStage {
            name: phase.to_string(),
            number: stage_number,
            started_at: Instant::now(),
        });
    }

    fn complete_active_stage(&mut self) -> Result<(), CliError> {
        let Some(active_stage) = self.active_stage.take() else {
            return Ok(());
        };

        let Some(adapter) = self.adapter.as_mut() else {
            return Ok(());
        };

        self.completed_stage_count = self.completed_stage_count.max(active_stage.number);

        adapter.observe(ProgressEvent::StageCompleted {
            stage: active_stage.name,
            status: "completed".to_string(),
            runtime_seconds: active_stage.started_at.elapsed().as_secs_f64(),
        });

        adapter
            .draw_latest()
            .map_err(|source| CliError::OutputProgress {
                source: Box::new(source),
            })
    }
}

pub(crate) struct StageProgressUpdate<'a> {
    pub(crate) phase: &'a str,
    pub(crate) stage_number: usize,
    pub(crate) completed_units: Option<usize>,
    pub(crate) total_units: Option<usize>,
    pub(crate) work_item_label: Option<String>,
    pub(crate) work_item_progress_percent: Option<usize>,
    pub(crate) planned_work_item_labels: Vec<String>,
    pub(crate) work_item_columns: Vec<ProgressWorkItemColumn>,
    pub(crate) work_item_metrics: Vec<ProgressWorkItemMetric>,
    pub(crate) latest_signal: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_advances_terminal_stage_statuses() {
        let mut progress = TerminalCommandProgress {
            adapter: Some(ProgressAdapter::hidden()),
            total_units: 2,
            active_stage: None,
            completed_stage_count: 0,
        };

        progress
            .adapter
            .as_mut()
            .expect("test adapter should be available")
            .observe(ProgressEvent::RunStarted {
                run_id: "test_run".to_string(),
                scope: "test".to_string(),
                input_count: 1,
                output_path: None,
                stage_sequence: vec!["load".to_string(), "write".to_string()],
                completed_stage_count: 0,
            });

        progress
            .observe("load", 1)
            .expect("first stage should render");

        assert_eq!(
            stage_statuses(&progress),
            vec!["Running".to_string(), "Pending".to_string()]
        );

        progress
            .observe("write", 2)
            .expect("second stage should render");

        assert_eq!(
            stage_statuses(&progress),
            vec!["Completed".to_string(), "Running".to_string()]
        );

        progress
            .complete_active_stage()
            .expect("active stage should complete");

        assert_eq!(
            stage_statuses(&progress),
            vec!["Completed".to_string(), "Completed".to_string()]
        );
    }

    #[test]
    fn complete_does_not_emit_completion_as_latest_signal() {
        let mut progress = TerminalCommandProgress {
            adapter: Some(ProgressAdapter::hidden()),
            total_units: 1,
            active_stage: None,
            completed_stage_count: 0,
        };

        progress
            .adapter
            .as_mut()
            .expect("test adapter should be available")
            .observe(ProgressEvent::RunStarted {
                run_id: "test_run".to_string(),
                scope: "test".to_string(),
                input_count: 1,
                output_path: None,
                stage_sequence: vec!["load".to_string()],
                completed_stage_count: 0,
            });

        progress.observe("load", 1).expect("stage should render");
        progress.complete().expect("run should complete");

        assert_ne!(
            progress
                .adapter
                .as_ref()
                .expect("test adapter should be available")
                .snapshot()
                .latest_signal
                .as_deref(),
            Some("completed")
        );
    }

    fn stage_statuses(progress: &TerminalCommandProgress) -> Vec<String> {
        progress
            .adapter
            .as_ref()
            .expect("test adapter should be available")
            .snapshot()
            .stages
            .iter()
            .map(|stage| format!("{:?}", stage.status))
            .collect()
    }
}
