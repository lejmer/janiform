use std::path::Path;
use std::time::Instant;

use janiform_terminal_ui::{
    ProgressAdapter, ProgressEvent, ProgressWorkItemColumn, ProgressWorkItemMetric,
    TerminalSummaryPolicy,
};
use serde::{Deserialize, Serialize};

use crate::error::WorkflowError;
use crate::progress::{VolatileProgressGate, bounded_progress_measure};
use crate::terminal::{
    complete_terminal_progress, emit_terminal_completion_summary, emit_terminal_start_summary,
};

use super::output::ForecastOutput;
use super::request::ForecastMeasure;

/// Forecast progress phase emitted to terminal and Python callback observers.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForecastProgressPhase {
    ResolvingHorizon,
    MaterializingIntervals,
    ProjectingExpectedPath,
    SolvingTerminalLaw,
    BuildingDistribution,
    WritingForecastBundle,
}

/// Structured forecast progress event.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ForecastProgressEvent {
    pub phase: ForecastProgressPhase,
    pub completed_units: usize,
    pub total_units: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_signal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_item_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_item_progress_percent: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_item_status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub planned_work_item_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub work_item_columns: Vec<ForecastProgressWorkItemColumn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub work_item_metrics: Vec<ForecastProgressWorkItemMetric>,
}

/// Serializable forecast progress work-item table column.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ForecastProgressWorkItemColumn {
    pub key: String,
    pub label: String,
}

impl ForecastProgressWorkItemColumn {
    pub(crate) fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }
}

/// Serializable forecast progress work-item metric.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ForecastProgressWorkItemMetric {
    pub key: String,
    pub value: String,
}

impl ForecastProgressWorkItemMetric {
    pub(crate) fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

impl ForecastProgressEvent {
    pub(crate) fn new(
        phase: ForecastProgressPhase,
        completed_units: usize,
        total_units: usize,
    ) -> Self {
        Self {
            phase,
            completed_units,
            total_units,
            latest_signal: None,
            work_item_label: None,
            work_item_progress_percent: None,
            work_item_status: None,
            planned_work_item_labels: Vec::new(),
            work_item_columns: Vec::new(),
            work_item_metrics: Vec::new(),
        }
    }

    pub(crate) fn with_latest_signal(mut self, latest_signal: impl Into<String>) -> Self {
        self.latest_signal = Some(latest_signal.into());

        self
    }

    pub(crate) fn with_work_plan(
        mut self,
        planned_work_item_labels: Vec<String>,
        work_item_columns: Vec<ForecastProgressWorkItemColumn>,
    ) -> Self {
        self.planned_work_item_labels = planned_work_item_labels;
        self.work_item_columns = work_item_columns;

        self
    }

    pub(crate) fn with_work_item(
        mut self,
        work_item_label: String,
        work_item_progress_percent: usize,
        work_item_metrics: Vec<ForecastProgressWorkItemMetric>,
    ) -> Self {
        self.work_item_label = Some(work_item_label);
        self.work_item_progress_percent = Some(work_item_progress_percent);
        self.work_item_metrics = work_item_metrics;

        self
    }

    fn has_work_item_plan(&self) -> bool {
        !self.planned_work_item_labels.is_empty()
    }
}

pub(super) fn start_forecast_progress(
    progress_adapter: Option<&mut ProgressAdapter>,
    run_id: &str,
    input_count: usize,
    output_path: Option<&Path>,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::RunStarted {
        run_id: run_id.to_string(),
        scope: "forecast".to_string(),
        input_count,
        output_path: output_path.map(janiform_terminal_ui::format_path),
        stage_sequence: forecast_progress_stage_sequence(),
        completed_stage_count: 0,
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

pub(super) fn observe_gated_forecast_terminal_event(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    progress_gate: &mut VolatileProgressGate<ForecastProgressPhase>,
    progress_error: &mut Option<WorkflowError>,
    event: ForecastProgressEvent,
) {
    if progress_error.is_some() || progress_adapter.is_none() {
        return;
    }
    if !event.has_work_item_plan()
        && !progress_gate.should_emit(
            Instant::now(),
            event.phase,
            event.completed_units,
            event.total_units,
        )
    {
        return;
    }

    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return;
    };

    if let Err(source) = observe_forecast_terminal_event(progress_adapter, &event) {
        *progress_error = Some(source);
    }
}

fn observe_forecast_terminal_event(
    progress_adapter: &mut ProgressAdapter,
    event: &ForecastProgressEvent,
) -> Result<(), WorkflowError> {
    progress_adapter.observe(ProgressEvent::StageProgress {
        phase: forecast_progress_phase_label(event.phase).to_string(),
        measure: bounded_progress_measure(event.completed_units, event.total_units)?,
        work_item_label: event.work_item_label.clone(),
        work_item_progress_percent: event.work_item_progress_percent,
        work_item_status: event.work_item_status.clone(),
        planned_work_item_labels: event.planned_work_item_labels.clone(),
        work_item_columns: event
            .work_item_columns
            .iter()
            .map(|column| ProgressWorkItemColumn::new(column.key.as_str(), column.label.as_str()))
            .collect(),
        work_item_metrics: event
            .work_item_metrics
            .iter()
            .map(|metric| ProgressWorkItemMetric::new(metric.key.as_str(), metric.value.as_str()))
            .collect(),
        latest_signal: event
            .latest_signal
            .clone()
            .or_else(|| Some(format!("{}/{}", event.completed_units, event.total_units))),
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

pub(super) fn complete_forecast_progress(
    progress_adapter: Option<&mut ProgressAdapter>,
    progress_error: Option<WorkflowError>,
) -> Result<(), WorkflowError> {
    if let Some(progress_error) = progress_error {
        return Err(progress_error);
    }

    let Some(progress_adapter) = progress_adapter else {
        return Ok(());
    };

    complete_terminal_progress(progress_adapter, "completed")
}

pub(super) fn complete_forecast_stage_with_warnings(
    progress_adapter: Option<&mut ProgressAdapter>,
    phase: ForecastProgressPhase,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::StageCompleted {
        stage: forecast_progress_phase_label(phase).to_string(),
        status: "completed with warnings".to_string(),
        runtime_seconds: 0.0,
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

pub(super) fn fail_forecast_progress(
    progress_adapter: Option<&mut ProgressAdapter>,
    reason: &str,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter else {
        return Ok(());
    };

    progress_adapter
        .fail_and_keep(reason)
        .map_err(|source| WorkflowError::OutputWrite { source })
}

pub(super) fn emit_forecast_start_summary(
    terminal_output: &Option<TerminalSummaryPolicy>,
    run_id: &str,
    measure: ForecastMeasure,
    horizon_interval_count: Option<usize>,
    output_path: Option<&Path>,
) -> Result<(), WorkflowError> {
    emit_terminal_start_summary(
        terminal_output,
        super::output::forecast_start_summary(run_id, measure, horizon_interval_count, output_path),
    )
}

pub(super) fn emit_forecast_completion_summary(
    terminal_output: &Option<TerminalSummaryPolicy>,
    output: &ForecastOutput,
) -> Result<(), WorkflowError> {
    emit_terminal_completion_summary(
        terminal_output,
        super::output::forecast_completion_summary(output),
    )
}

fn forecast_progress_stage_sequence() -> Vec<String> {
    [
        ForecastProgressPhase::ResolvingHorizon,
        ForecastProgressPhase::MaterializingIntervals,
        ForecastProgressPhase::ProjectingExpectedPath,
        ForecastProgressPhase::SolvingTerminalLaw,
        ForecastProgressPhase::BuildingDistribution,
        ForecastProgressPhase::WritingForecastBundle,
    ]
    .iter()
    .map(|phase| forecast_progress_phase_label(*phase).to_string())
    .collect()
}

fn forecast_progress_phase_label(phase: ForecastProgressPhase) -> &'static str {
    match phase {
        ForecastProgressPhase::ResolvingHorizon => "Resolving horizon",
        ForecastProgressPhase::MaterializingIntervals => "Materializing intervals",
        ForecastProgressPhase::ProjectingExpectedPath => "Projecting expected path",
        ForecastProgressPhase::SolvingTerminalLaw => "Solving terminal law",
        ForecastProgressPhase::BuildingDistribution => "Building distribution",
        ForecastProgressPhase::WritingForecastBundle => "Writing forecast bundle",
    }
}
