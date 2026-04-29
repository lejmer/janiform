use janiform_terminal_ui::{ProgressWorkItemColumn, ProgressWorkItemMetric};

use super::super::artifacts::SimulationPathManifest;
use super::super::progress::{
    StylizedFactsValidationProgressEvent, StylizedFactsValidationProgressPhase,
};
use super::super::workflow::StylizedFactsValidationWorkflowObserver;
use crate::progress::progress_percent;

pub(super) fn emit_simulation_plan(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    seeds: &[u64],
    worker_count: usize,
    total_interval_units: usize,
) {
    observer(StylizedFactsValidationProgressEvent {
        phase: StylizedFactsValidationProgressPhase::SimulateSeededPath,
        label: "simulate seeded paths".to_string(),
        step: 3,
        total_steps: super::super::execution::VALIDATION_PHASE_COUNT,
        completed_units: Some(0),
        total_units: Some(total_interval_units),
        work_item_label: None,
        work_item_progress_percent: None,
        planned_work_item_labels: seeds
            .iter()
            .enumerate()
            .map(|(path_position, _)| simulation_path_label(path_position))
            .collect(),
        work_item_columns: simulation_work_item_columns(),
        work_item_metrics: Vec::new(),
        latest_signal: Some(format!(
            "simulating {} on {}",
            format_path_count(seeds.len()),
            format_cpu_count(worker_count)
        )),
    });
}

pub(super) fn emit_simulation_started(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    path_position: usize,
    seed: u64,
    path_total_intervals: usize,
    total_path_count: usize,
    completed_units: usize,
    total_units: usize,
    worker_count: usize,
) {
    emit_simulation_progress(
        observer,
        simulation_path_label(path_position),
        Some(0),
        completed_units,
        total_units,
        vec![
            ProgressWorkItemMetric::new("seed", seed.to_string()),
            ProgressWorkItemMetric::new("intervals", format!("0/{path_total_intervals}")),
            ProgressWorkItemMetric::new("transitions", "-"),
            ProgressWorkItemMetric::new("events", "-"),
            ProgressWorkItemMetric::new("cpu", worker_count.to_string()),
        ],
        format!(
            "simulating path {}/{} on {}",
            path_position + 1,
            total_path_count,
            format_cpu_count(worker_count)
        ),
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_simulation_interval_progress(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    path_position: usize,
    seed: u64,
    path_completed_intervals: usize,
    path_total_intervals: usize,
    completed_units: usize,
    total_units: usize,
    worker_count: usize,
) {
    emit_simulation_progress(
        observer,
        simulation_path_label(path_position),
        Some(progress_percent(
            path_completed_intervals,
            path_total_intervals,
        )),
        completed_units,
        total_units,
        vec![
            ProgressWorkItemMetric::new("seed", seed.to_string()),
            ProgressWorkItemMetric::new(
                "intervals",
                format!("{path_completed_intervals}/{path_total_intervals}"),
            ),
            ProgressWorkItemMetric::new("transitions", "-"),
            ProgressWorkItemMetric::new("events", "-"),
            ProgressWorkItemMetric::new("cpu", worker_count.to_string()),
        ],
        format!(
            "simulating path {} interval {}/{} on {}",
            path_position + 1,
            path_completed_intervals,
            path_total_intervals,
            format_cpu_count(worker_count)
        ),
    );
}

pub(super) fn emit_simulation_completed(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    manifest: &SimulationPathManifest,
    path_total_intervals: usize,
    completed_units: usize,
    total_units: usize,
    completed_path_count: usize,
    total_path_count: usize,
    worker_count: usize,
) {
    emit_simulation_progress(
        observer,
        simulation_path_label(manifest.path_position),
        Some(100),
        completed_units,
        total_units,
        vec![
            ProgressWorkItemMetric::new("seed", manifest.seed.to_string()),
            ProgressWorkItemMetric::new(
                "intervals",
                format!("{path_total_intervals}/{path_total_intervals}"),
            ),
            ProgressWorkItemMetric::new("transitions", manifest.transition_count.to_string()),
            ProgressWorkItemMetric::new("events", manifest.event_count.to_string()),
            ProgressWorkItemMetric::new("cpu", worker_count.to_string()),
        ],
        format!("simulated {completed_path_count}/{total_path_count} paths"),
    );
}

fn emit_simulation_progress(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    work_item_label: String,
    work_item_progress_percent: Option<usize>,
    completed_units: usize,
    total_units: usize,
    work_item_metrics: Vec<ProgressWorkItemMetric>,
    latest_signal: String,
) {
    observer(StylizedFactsValidationProgressEvent {
        phase: StylizedFactsValidationProgressPhase::SimulateSeededPath,
        label: "simulate seeded paths".to_string(),
        step: 3,
        total_steps: super::super::execution::VALIDATION_PHASE_COUNT,
        completed_units: Some(completed_units),
        total_units: Some(total_units),
        work_item_label: Some(work_item_label),
        work_item_progress_percent,
        planned_work_item_labels: Vec::new(),
        work_item_columns: simulation_work_item_columns(),
        work_item_metrics,
        latest_signal: Some(latest_signal),
    });
}

fn simulation_work_item_columns() -> Vec<ProgressWorkItemColumn> {
    vec![
        ProgressWorkItemColumn::new("seed", "Seed"),
        ProgressWorkItemColumn::new("intervals", "Intervals"),
        ProgressWorkItemColumn::new("transitions", "Transitions"),
        ProgressWorkItemColumn::new("events", "Events"),
        ProgressWorkItemColumn::new("cpu", "CPU"),
    ]
}

fn simulation_path_label(path_position: usize) -> String {
    format!("path {}", path_position + 1)
}

fn format_path_count(path_count: usize) -> String {
    match path_count {
        1 => "1 path".to_string(),
        _ => format!("{path_count} paths"),
    }
}

fn format_cpu_count(cpu_count: usize) -> String {
    match cpu_count {
        1 => "1 CPU".to_string(),
        _ => format!("{cpu_count} CPUs"),
    }
}

#[cfg(test)]
mod tests {
    use super::{emit_simulation_interval_progress, emit_simulation_plan, emit_simulation_started};

    #[test]
    fn simulation_progress_reports_interval_units_inside_single_path() {
        let mut events = Vec::new();

        emit_simulation_plan(&mut |event| events.push(event), &[7], 1, 10);
        emit_simulation_started(&mut |event| events.push(event), 0, 7, 10, 1, 0, 10, 1);
        emit_simulation_interval_progress(&mut |event| events.push(event), 0, 7, 4, 10, 4, 10, 1);

        let progress_event = events
            .last()
            .expect("simulation interval progress should be emitted");

        assert_eq!(progress_event.completed_units, Some(4));
        assert_eq!(progress_event.total_units, Some(10));
        assert_eq!(progress_event.work_item_progress_percent, Some(40));
        assert!(
            progress_event
                .work_item_metrics
                .iter()
                .any(|metric| metric.key == "intervals" && metric.value == "4/10")
        );
    }
}
