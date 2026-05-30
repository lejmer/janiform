use janiform_terminal_ui::{ProgressWorkItemColumn, ProgressWorkItemMetric};

use super::super::artifacts::SimulationPathManifest;
use super::super::labels::stylized_facts_validation_phase_label;
use super::super::progress::{
    StylizedFactsValidationProgressEvent, StylizedFactsValidationProgressPhase,
};
use super::super::workflow::StylizedFactsValidationWorkflowObserver;
use crate::progress::{format_cpu_count, progress_percent};

pub(super) fn emit_simulation_plan(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    seeds: &[u64],
    worker_count: usize,
    total_interval_units: usize,
) {
    observer(StylizedFactsValidationProgressEvent {
        phase: StylizedFactsValidationProgressPhase::SimulateSeededPath,
        label: stylized_facts_validation_phase_label(
            StylizedFactsValidationProgressPhase::SimulateSeededPath,
        )
        .to_string(),
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

pub(super) struct SimulationStartedProgressInput {
    pub(super) path_position: usize,
    pub(super) seed: u64,
    pub(super) path_total_intervals: usize,
    pub(super) total_path_count: usize,
    pub(super) completed_units: usize,
    pub(super) total_units: usize,
    pub(super) worker_count: usize,
}

pub(super) fn emit_simulation_started(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    input: SimulationStartedProgressInput,
) {
    emit_simulation_progress(
        observer,
        simulation_path_label(input.path_position),
        Some(0),
        input.completed_units,
        input.total_units,
        vec![
            ProgressWorkItemMetric::new("seed", input.seed.to_string()),
            ProgressWorkItemMetric::new("intervals", format!("0/{}", input.path_total_intervals)),
            ProgressWorkItemMetric::new("transitions", "-"),
            ProgressWorkItemMetric::new("events", "-"),
            ProgressWorkItemMetric::new("cpu", input.worker_count.to_string()),
        ],
        format!(
            "simulating path {}/{} on {}",
            input.path_position + 1,
            input.total_path_count,
            format_cpu_count(input.worker_count)
        ),
    );
}

pub(super) struct SimulationIntervalProgressInput {
    pub(super) path_position: usize,
    pub(super) seed: u64,
    pub(super) path_completed_intervals: usize,
    pub(super) path_total_intervals: usize,
    pub(super) completed_units: usize,
    pub(super) total_units: usize,
    pub(super) worker_count: usize,
}

pub(super) fn emit_simulation_interval_progress(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    input: SimulationIntervalProgressInput,
) {
    emit_simulation_progress(
        observer,
        simulation_path_label(input.path_position),
        Some(progress_percent(
            input.path_completed_intervals,
            input.path_total_intervals,
        )),
        input.completed_units,
        input.total_units,
        vec![
            ProgressWorkItemMetric::new("seed", input.seed.to_string()),
            ProgressWorkItemMetric::new(
                "intervals",
                format!(
                    "{}/{}",
                    input.path_completed_intervals, input.path_total_intervals
                ),
            ),
            ProgressWorkItemMetric::new("transitions", "-"),
            ProgressWorkItemMetric::new("events", "-"),
            ProgressWorkItemMetric::new("cpu", input.worker_count.to_string()),
        ],
        format!(
            "simulating path {} interval {}/{} on {}",
            input.path_position + 1,
            input.path_completed_intervals,
            input.path_total_intervals,
            format_cpu_count(input.worker_count)
        ),
    );
}

pub(super) struct SimulationCompletedProgressInput<'a> {
    pub(super) manifest: &'a SimulationPathManifest,
    pub(super) path_total_intervals: usize,
    pub(super) completed_units: usize,
    pub(super) total_units: usize,
    pub(super) completed_path_count: usize,
    pub(super) total_path_count: usize,
    pub(super) worker_count: usize,
}

pub(super) fn emit_simulation_completed(
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    input: SimulationCompletedProgressInput<'_>,
) {
    emit_simulation_progress(
        observer,
        simulation_path_label(input.manifest.path_position),
        Some(100),
        input.completed_units,
        input.total_units,
        vec![
            ProgressWorkItemMetric::new("seed", input.manifest.seed.to_string()),
            ProgressWorkItemMetric::new(
                "intervals",
                format!(
                    "{}/{}",
                    input.path_total_intervals, input.path_total_intervals
                ),
            ),
            ProgressWorkItemMetric::new("transitions", input.manifest.transition_count.to_string()),
            ProgressWorkItemMetric::new("events", input.manifest.event_count.to_string()),
            ProgressWorkItemMetric::new("cpu", input.worker_count.to_string()),
        ],
        format!(
            "simulated {}/{} paths",
            input.completed_path_count, input.total_path_count
        ),
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
        label: stylized_facts_validation_phase_label(
            StylizedFactsValidationProgressPhase::SimulateSeededPath,
        )
        .to_string(),
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

#[cfg(test)]
mod tests {
    use super::{
        SimulationIntervalProgressInput, SimulationStartedProgressInput,
        emit_simulation_interval_progress, emit_simulation_plan, emit_simulation_started,
    };

    #[test]
    fn simulation_progress_reports_interval_units_inside_single_path() {
        let mut events = Vec::new();

        emit_simulation_plan(&mut |event| events.push(event), &[7], 1, 10);
        emit_simulation_started(
            &mut |event| events.push(event),
            SimulationStartedProgressInput {
                path_position: 0,
                seed: 7,
                path_total_intervals: 10,
                total_path_count: 1,
                completed_units: 0,
                total_units: 10,
                worker_count: 1,
            },
        );
        emit_simulation_interval_progress(
            &mut |event| events.push(event),
            SimulationIntervalProgressInput {
                path_position: 0,
                seed: 7,
                path_completed_intervals: 4,
                path_total_intervals: 10,
                completed_units: 4,
                total_units: 10,
                worker_count: 1,
            },
        );

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
