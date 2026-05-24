use std::time::Instant;

use janiform::calibration::current_state::CurrentStateEstimationBatchProgressEvent;
use janiform::pricing::PricingProgressEvent;
use janiform_terminal_ui::{
    ProgressAdapter, ProgressEvent, ProgressMeasure, ProgressWorkItemColumn, ProgressWorkItemMetric,
};

use crate::error::WorkflowError;

use super::super::progress::pricing_progress_phase_label;
use super::types::{
    LOAD_OBSERVED_DATA_STAGE, PHYSICAL_DISTRIBUTION_DIAGNOSTICS_STAGE,
    PREPARE_VALUATION_STATES_STAGE, PRICE_VALUATION_SURFACE_STAGE, WRITE_ARTIFACTS_STAGE,
};

pub(super) struct ObservedWorkItemUpdate {
    work_item_label: Option<String>,
    work_item_progress_percent: Option<usize>,
    planned_work_item_labels: Vec<String>,
    work_item_columns: Vec<ProgressWorkItemColumn>,
    work_item_metrics: Vec<ProgressWorkItemMetric>,
}

impl ObservedWorkItemUpdate {
    pub(super) fn none() -> Self {
        Self {
            work_item_label: None,
            work_item_progress_percent: None,
            planned_work_item_labels: Vec::new(),
            work_item_columns: Vec::new(),
            work_item_metrics: Vec::new(),
        }
    }

    pub(super) fn planned(
        planned_work_item_labels: Vec<String>,
        work_item_columns: Vec<ProgressWorkItemColumn>,
    ) -> Self {
        Self {
            work_item_label: None,
            work_item_progress_percent: None,
            planned_work_item_labels,
            work_item_columns,
            work_item_metrics: Vec::new(),
        }
    }

    pub(super) fn active(
        work_item_label: String,
        work_item_progress_percent: usize,
        work_item_columns: Vec<ProgressWorkItemColumn>,
        work_item_metrics: Vec<ProgressWorkItemMetric>,
    ) -> Self {
        Self {
            work_item_label: Some(work_item_label),
            work_item_progress_percent: Some(work_item_progress_percent),
            planned_work_item_labels: Vec::new(),
            work_item_columns,
            work_item_metrics,
        }
    }
}

pub(super) fn start_observed_pricing_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    run_id: &str,
    output_path: Option<&std::path::Path>,
    include_physical_distribution_diagnostics: bool,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::RunStarted {
        run_id: run_id.to_string(),
        scope: "Observed options".to_string(),
        input_count: 1,
        output_path: output_path.map(janiform_terminal_ui::format_path),
        stage_sequence: observed_pricing_stage_sequence(include_physical_distribution_diagnostics),
        completed_stage_count: 0,
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

pub(super) fn start_observed_pricing_stage(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    stage_number: usize,
    stage_count: usize,
    stage: &str,
) -> Result<Instant, WorkflowError> {
    let started_at = Instant::now();

    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(started_at);
    };

    progress_adapter.observe(ProgressEvent::StageStarted {
        stage: stage.to_string(),
        stage_number,
        stage_count,
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })?;

    Ok(started_at)
}

pub(super) fn observed_pricing_stage_count(
    include_physical_distribution_diagnostics: bool,
) -> usize {
    observed_pricing_stage_sequence(include_physical_distribution_diagnostics).len()
}

fn observed_pricing_stage_sequence(include_physical_distribution_diagnostics: bool) -> Vec<String> {
    let mut stages = vec![
        LOAD_OBSERVED_DATA_STAGE.to_string(),
        PREPARE_VALUATION_STATES_STAGE.to_string(),
        PRICE_VALUATION_SURFACE_STAGE.to_string(),
    ];

    if include_physical_distribution_diagnostics {
        stages.push(PHYSICAL_DISTRIBUTION_DIAGNOSTICS_STAGE.to_string());
    }

    stages.push(WRITE_ARTIFACTS_STAGE.to_string());

    stages
}

pub(super) fn complete_observed_pricing_stage(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    stage: &str,
    started_at: Instant,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::StageCompleted {
        stage: stage.to_string(),
        status: "completed".to_string(),
        runtime_seconds: started_at.elapsed().as_secs_f64(),
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

pub(super) fn emit_observed_stage_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    phase: &str,
    measure: ProgressMeasure,
    latest_signal: Option<String>,
    work_item_update: ObservedWorkItemUpdate,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::StageProgress {
        phase: phase.to_string(),
        measure,
        work_item_label: work_item_update.work_item_label,
        work_item_progress_percent: work_item_update.work_item_progress_percent,
        work_item_status: None,
        planned_work_item_labels: work_item_update.planned_work_item_labels,
        work_item_columns: work_item_update.work_item_columns,
        work_item_metrics: work_item_update.work_item_metrics,
        latest_signal,
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

pub(super) fn emit_valuation_state_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    event: CurrentStateEstimationBatchProgressEvent,
) -> Result<(), WorkflowError> {
    let completed_units = event.consumed_observation_count + event.completed_state_count;
    let total_units = event.total_observation_count + event.total_state_count;

    emit_observed_stage_progress(
        progress_adapter,
        "prepare valuation states",
        ProgressMeasure::bounded(completed_units, total_units)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some(format!(
            "replayed {}/{} ticks, prepared {}/{} valuation states",
            event.consumed_observation_count,
            event.total_observation_count,
            event.completed_state_count,
            event.total_state_count
        )),
        ObservedWorkItemUpdate::none(),
    )
}

pub(super) fn emit_single_snapshot_pricing_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    event: &PricingProgressEvent,
) -> Result<(), WorkflowError> {
    emit_observed_stage_progress(
        progress_adapter,
        pricing_progress_phase_label(event.phase),
        ProgressMeasure::bounded(event.completed_units, event.total_units)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some(format!(
            "{} {}",
            pricing_progress_phase_label(event.phase),
            pricing_event_units(event)
        )),
        ObservedWorkItemUpdate::none(),
    )
}

pub(super) fn pricing_event_units(event: &PricingProgressEvent) -> String {
    format!("{}/{}", event.completed_units, event.total_units)
}
