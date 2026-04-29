use janiform::verification::TailSkewDatasetRole;
use janiform_terminal_ui::{ProgressWorkItemColumn, ProgressWorkItemMetric};

use super::super::progress::{
    StylizedFactsValidationProgressEvent, StylizedFactsValidationProgressPhase,
};
use super::super::workflow::StylizedFactsValidationWorkflowObserver;
use crate::progress::progress_percent;

struct MetricProgressUpdate {
    completed_units: usize,
    total_units: usize,
    planned_work_item_labels: Vec<String>,
    latest_signal: String,
    work_item_label: Option<String>,
    work_item_progress_percent: Option<usize>,
    work_item_metrics: Vec<ProgressWorkItemMetric>,
}

pub(super) fn emit_metric_plan(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    planned_work_item_labels: Vec<String>,
    interval_count: usize,
    total_units: usize,
    cpu_count: usize,
) {
    let worker_count = effective_worker_count(cpu_count, total_units);

    emit_metric_progress(
        observer,
        MetricProgressUpdate {
            completed_units: 0,
            total_units,
            planned_work_item_labels,
            latest_signal: format!(
                "computing {} as {} on {}",
                format_interval_count(interval_count),
                format_metric_task_count(total_units),
                format_cpu_count(worker_count)
            ),
            work_item_label: None,
            work_item_progress_percent: None,
            work_item_metrics: Vec::new(),
        },
    );
}

pub(super) fn emit_metric_started(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    interval_label: &str,
    completed_units: usize,
    total_units: usize,
    cpu_count: usize,
    metric_task_count: usize,
) {
    let worker_count = effective_worker_count(cpu_count, metric_task_count);

    emit_metric_progress(
        observer,
        MetricProgressUpdate {
            completed_units,
            total_units,
            planned_work_item_labels: Vec::new(),
            latest_signal: format!(
                "computing {interval_label} as {} on {}",
                format_metric_task_count(metric_task_count),
                format_cpu_count(worker_count)
            ),
            work_item_label: Some(interval_label.to_string()),
            work_item_progress_percent: Some(0),
            work_item_metrics: vec![
                ProgressWorkItemMetric::new("family", "tails/skew"),
                ProgressWorkItemMetric::new("dataset", "-"),
                ProgressWorkItemMetric::new("metric", "-"),
                ProgressWorkItemMetric::new("checks", "-"),
                ProgressWorkItemMetric::new("tasks", metric_task_count.to_string()),
                ProgressWorkItemMetric::new("cpu", worker_count.to_string()),
            ],
        },
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_metric_task_completed(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    interval_label: &str,
    dataset_role: TailSkewDatasetRole,
    metric_id: &str,
    completed_units: usize,
    total_units: usize,
    interval_completed_units: usize,
    interval_total_units: usize,
    cpu_count: usize,
) {
    let worker_count = effective_worker_count(cpu_count, interval_total_units);
    let dataset_label = dataset_role_label(dataset_role);

    emit_metric_progress(
        observer,
        MetricProgressUpdate {
            completed_units,
            total_units,
            planned_work_item_labels: Vec::new(),
            latest_signal: format!("computed {dataset_label} {metric_id} for {interval_label}"),
            work_item_label: Some(interval_label.to_string()),
            work_item_progress_percent: Some(progress_percent(
                interval_completed_units,
                interval_total_units,
            )),
            work_item_metrics: vec![
                ProgressWorkItemMetric::new("family", "tails/skew"),
                ProgressWorkItemMetric::new("dataset", dataset_label),
                ProgressWorkItemMetric::new("metric", metric_id),
                ProgressWorkItemMetric::new(
                    "checks",
                    format!("{interval_completed_units}/{interval_total_units}"),
                ),
                ProgressWorkItemMetric::new("tasks", interval_total_units.to_string()),
                ProgressWorkItemMetric::new("cpu", worker_count.to_string()),
            ],
        },
    );
}

pub(super) fn emit_metric_completed(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    interval_label: &str,
    metric_count: usize,
    completed_units: usize,
    total_units: usize,
    cpu_count: usize,
    metric_task_count: usize,
) {
    let worker_count = effective_worker_count(cpu_count, metric_task_count);

    emit_metric_progress(
        observer,
        MetricProgressUpdate {
            completed_units,
            total_units,
            planned_work_item_labels: Vec::new(),
            latest_signal: format!("computed {completed_units}/{total_units} metric tasks"),
            work_item_label: Some(interval_label.to_string()),
            work_item_progress_percent: Some(100),
            work_item_metrics: vec![
                ProgressWorkItemMetric::new("family", "tails/skew"),
                ProgressWorkItemMetric::new("dataset", "-"),
                ProgressWorkItemMetric::new("metric", "-"),
                ProgressWorkItemMetric::new("checks", metric_count.to_string()),
                ProgressWorkItemMetric::new("tasks", metric_task_count.to_string()),
                ProgressWorkItemMetric::new("cpu", worker_count.to_string()),
            ],
        },
    );
}

fn emit_metric_progress(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    update: MetricProgressUpdate,
) {
    observer(StylizedFactsValidationProgressEvent {
        phase: StylizedFactsValidationProgressPhase::ComputeMetrics,
        label: "compute return-based metrics".to_string(),
        step: super::super::execution::COMPUTE_METRICS_PHASE_STEP,
        total_steps: super::super::execution::VALIDATION_PHASE_COUNT,
        completed_units: Some(update.completed_units),
        total_units: Some(update.total_units),
        work_item_label: update.work_item_label,
        work_item_progress_percent: update.work_item_progress_percent,
        planned_work_item_labels: update.planned_work_item_labels,
        work_item_columns: vec![
            ProgressWorkItemColumn::new("family", "Family"),
            ProgressWorkItemColumn::new("dataset", "Dataset"),
            ProgressWorkItemColumn::new("metric", "Metric"),
            ProgressWorkItemColumn::new("checks", "Checks"),
            ProgressWorkItemColumn::new("tasks", "Tasks"),
            ProgressWorkItemColumn::new("cpu", "CPU"),
        ],
        work_item_metrics: update.work_item_metrics,
        latest_signal: Some(update.latest_signal),
    });
}

fn format_interval_count(interval_count: usize) -> String {
    match interval_count {
        1 => "1 interval".to_string(),
        _ => format!("{interval_count} intervals"),
    }
}

fn effective_worker_count(cpu_count: usize, task_count: usize) -> usize {
    cpu_count.min(task_count.max(1))
}

fn format_metric_task_count(task_count: usize) -> String {
    match task_count {
        1 => "1 metric task".to_string(),
        _ => format!("{task_count} metric tasks"),
    }
}

fn format_cpu_count(cpu_count: usize) -> String {
    match cpu_count {
        1 => "1 CPU".to_string(),
        _ => format!("{cpu_count} CPUs"),
    }
}

fn dataset_role_label(dataset_role: TailSkewDatasetRole) -> &'static str {
    match dataset_role {
        TailSkewDatasetRole::Empirical => "empirical",
        TailSkewDatasetRole::Simulated => "simulated",
    }
}
