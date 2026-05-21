use chrono::{DateTime, Utc};
use janiform::pricing::{PricingProgressEvent, PricingProgressPhase};
use janiform_terminal_ui::{
    ProgressAdapter, ProgressMeasure, ProgressWorkItemColumn, ProgressWorkItemMetric,
};

use crate::error::WorkflowError;
use crate::progress::{
    ProgressUnitTracker, completed_tracked_progress_units, format_cpu_count, progress_count_label,
    progress_percent, progress_position_label, total_tracked_progress_units,
};

use super::super::progress::pricing_progress_phase_label;
use super::super::validation::pricing_validation_error;
use super::progress::{ObservedWorkItemUpdate, emit_observed_stage_progress, pricing_event_units};
use super::types::{ObservedOptionsSnapshotResult, SURFACE_TASK_FINALIZATION_UNIT};

pub(super) enum SurfacePricingMessage {
    Started {
        worker_index: usize,
        task_position: usize,
        valuation_time_utc_nanos: i64,
        node_count: usize,
        total_interval_count: usize,
    },
    Progress {
        worker_index: usize,
        event: Box<PricingProgressEvent>,
    },
}

#[derive(Clone)]
pub(super) struct SurfaceWorkerState {
    pub(super) task_position: usize,
    pub(super) valuation_time_utc_nanos: i64,
    pub(super) node_count: usize,
    pub(super) completed_intervals: usize,
    pub(super) total_intervals: usize,
    pub(super) completed_progress_units: usize,
    pub(super) total_progress_units: usize,
    pub(super) progress_percent: usize,
    pub(super) interval_units: String,
    pub(super) row_units: String,
    pub(super) phase: String,
}

impl SurfaceWorkerState {
    pub(super) fn started(
        task_position: usize,
        valuation_time_utc_nanos: i64,
        node_count: usize,
        total_intervals: usize,
    ) -> Self {
        Self {
            task_position,
            valuation_time_utc_nanos,
            node_count,
            completed_intervals: 0,
            total_intervals,
            completed_progress_units: 0,
            total_progress_units: surface_task_progress_total(total_intervals),
            progress_percent: 0,
            interval_units: "-".to_string(),
            row_units: "-".to_string(),
            phase: "starting".to_string(),
        }
    }

    pub(super) fn apply_pricing_event(&mut self, event: &PricingProgressEvent) {
        if let Some(total_intervals) = event.total_intervals {
            self.total_intervals = total_intervals;
        }

        self.completed_intervals = self
            .completed_intervals
            .max(surface_worker_event_completed_intervals(event));
        self.total_progress_units = surface_task_progress_total(self.total_intervals);
        self.completed_progress_units = self.completed_intervals.min(self.total_intervals);
        self.progress_percent =
            progress_percent(self.completed_progress_units, self.total_progress_units);
        self.interval_units = surface_worker_interval_units(event);
        self.row_units = surface_worker_row_units(event);
        self.phase = pricing_progress_phase_label(event.phase).to_string();
    }

    pub(super) fn complete(&mut self) {
        self.completed_intervals = self.total_intervals;
        self.completed_progress_units = self.total_progress_units;
        self.progress_percent = 100;
        self.interval_units = "-".to_string();
        self.row_units = "-".to_string();
        self.phase = "done".to_string();
    }
}

pub(super) struct SurfaceWorkerProgressInput {
    pub(super) worker_index: usize,
    pub(super) task_position: usize,
    pub(super) task_count: usize,
    pub(super) completed_progress_units: usize,
    pub(super) total_progress_units: usize,
    pub(super) worker_count: usize,
    pub(super) valuation_time_utc_nanos: i64,
    pub(super) node_count: usize,
    pub(super) total_interval_count: usize,
}

pub(super) struct SurfaceWorkerPricingProgressInput {
    pub(super) worker_index: usize,
    pub(super) task_count: usize,
    pub(super) completed_progress_units: usize,
    pub(super) total_progress_units: usize,
    pub(super) worker_count: usize,
    pub(super) worker_state: SurfaceWorkerState,
}

pub(super) struct SurfaceWorkerCompletedInput {
    pub(super) worker_index: usize,
    pub(super) task_count: usize,
    pub(super) completed_task_count: usize,
    pub(super) completed_progress_units: usize,
    pub(super) total_progress_units: usize,
    pub(super) worker_count: usize,
    pub(super) worker_state: SurfaceWorkerState,
}

pub(super) struct SurfacePricingResultCollector<'collector, 'adapter> {
    progress_adapter: &'collector mut Option<&'adapter mut ProgressAdapter>,
    worker_states: Vec<Option<SurfaceWorkerState>>,
    task_progress_units: Vec<ProgressUnitTracker>,
    completed_task_count: usize,
    results: Vec<Option<ObservedOptionsSnapshotResult>>,
    task_count: usize,
    worker_count: usize,
    total_progress_units: usize,
}

impl<'collector, 'adapter> SurfacePricingResultCollector<'collector, 'adapter> {
    pub(super) fn new(
        task_count: usize,
        worker_count: usize,
        task_interval_counts: &[usize],
        progress_adapter: &'collector mut Option<&'adapter mut ProgressAdapter>,
    ) -> Self {
        let task_progress_totals = surface_task_progress_totals(task_interval_counts);
        let task_progress_units =
            ProgressUnitTracker::planned_for_totals(task_progress_totals.as_slice());
        let total_progress_units = total_tracked_progress_units(task_progress_units.as_slice());

        Self {
            progress_adapter,
            worker_states: (0..worker_count).map(|_| None).collect(),
            task_progress_units,
            completed_task_count: 0,
            results: (0..task_count).map(|_| None).collect(),
            task_count,
            worker_count,
            total_progress_units,
        }
    }

    pub(super) fn record_progress(
        &mut self,
        message: SurfacePricingMessage,
    ) -> Result<(), WorkflowError> {
        match message {
            SurfacePricingMessage::Started {
                worker_index,
                task_position,
                valuation_time_utc_nanos,
                node_count,
                total_interval_count,
            } => {
                let Some(worker_state) = self.worker_states.get_mut(worker_index) else {
                    return Err(pricing_validation_error(
                        "pricing_progress.worker_index",
                        "referenced an unknown worker",
                    ));
                };

                *worker_state = Some(SurfaceWorkerState::started(
                    task_position,
                    valuation_time_utc_nanos,
                    node_count,
                    total_interval_count,
                ));

                emit_surface_worker_started(
                    self.progress_adapter,
                    SurfaceWorkerProgressInput {
                        worker_index,
                        task_position,
                        task_count: self.task_count,
                        completed_progress_units: completed_tracked_progress_units(
                            self.task_progress_units.as_slice(),
                        ),
                        total_progress_units: self.total_progress_units,
                        worker_count: self.worker_count,
                        valuation_time_utc_nanos,
                        node_count,
                        total_interval_count,
                    },
                )?;
            }
            SurfacePricingMessage::Progress {
                worker_index,
                event,
            } => {
                let worker_state = self
                    .worker_states
                    .get_mut(worker_index)
                    .and_then(Option::as_mut)
                    .ok_or_else(|| {
                        pricing_validation_error(
                            "pricing_progress.worker_index",
                            "received worker progress before worker start",
                        )
                    })?;
                worker_state.apply_pricing_event(&event);

                let Some(task_progress) =
                    self.task_progress_units.get_mut(worker_state.task_position)
                else {
                    return Err(pricing_validation_error(
                        "pricing_progress.task_position",
                        "referenced an unknown valuation task",
                    ));
                };

                task_progress.record(worker_state.completed_progress_units);

                emit_surface_worker_progress(
                    self.progress_adapter,
                    SurfaceWorkerPricingProgressInput {
                        worker_index,
                        task_count: self.task_count,
                        completed_progress_units: completed_tracked_progress_units(
                            self.task_progress_units.as_slice(),
                        ),
                        total_progress_units: self.total_progress_units,
                        worker_count: self.worker_count,
                        // Intentional copy because progress events carry a point-in-time
                        // worker snapshot while the worker keeps advancing.
                        worker_state: worker_state.clone(),
                    },
                )?;
            }
        }

        Ok(())
    }

    pub(super) fn record_completed(
        &mut self,
        worker_index: usize,
        task_position: usize,
        result: Result<ObservedOptionsSnapshotResult, WorkflowError>,
    ) -> Result<(), WorkflowError> {
        let worker_state = self
            .worker_states
            .get_mut(worker_index)
            .and_then(Option::as_mut)
            .ok_or_else(|| {
                pricing_validation_error(
                    "pricing_progress.worker_index",
                    "received worker completion before worker start",
                )
            })?;
        worker_state.complete();

        let Some(task_progress) = self.task_progress_units.get_mut(task_position) else {
            return Err(pricing_validation_error(
                "pricing_progress.task_position",
                "referenced an unknown valuation task",
            ));
        };

        task_progress.complete();

        // Intentional copy because the completion event needs a stable worker
        // snapshot after the mutable worker-state borrow ends.
        let worker_state = worker_state.clone();

        let result = result?;

        self.completed_task_count += 1;

        let Some(output_slot) = self.results.get_mut(task_position) else {
            return Err(pricing_validation_error(
                "pricing_progress.task_position",
                "referenced an unknown valuation task",
            ));
        };

        *output_slot = Some(result);

        emit_surface_worker_completed(
            self.progress_adapter,
            SurfaceWorkerCompletedInput {
                worker_index,
                task_count: self.task_count,
                completed_task_count: self.completed_task_count,
                completed_progress_units: completed_tracked_progress_units(
                    self.task_progress_units.as_slice(),
                ),
                total_progress_units: self.total_progress_units,
                worker_count: self.worker_count,
                worker_state,
            },
        )
    }

    pub(super) fn finish(self) -> Result<Vec<ObservedOptionsSnapshotResult>, WorkflowError> {
        if self.completed_task_count != self.task_count {
            return Err(WorkflowError::PricingExecutionFailed {
                message: format!(
                    "observed surface pricing completed {}/{} valuation tasks",
                    self.completed_task_count, self.task_count
                ),
            });
        }

        self.results
            .into_iter()
            .map(|result| {
                result.ok_or_else(|| WorkflowError::PricingExecutionFailed {
                    message: "observed surface pricing did not return every valuation task"
                        .to_string(),
                })
            })
            .collect()
    }
}

pub(super) fn emit_surface_pricing_plan(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    task_count: usize,
    worker_count: usize,
    task_interval_counts: &[usize],
) -> Result<(), WorkflowError> {
    let task_progress_totals = surface_task_progress_totals(task_interval_counts);
    let task_progress_units =
        ProgressUnitTracker::planned_for_totals(task_progress_totals.as_slice());
    let total_progress_units = total_tracked_progress_units(task_progress_units.as_slice());

    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        surface_progress_measure(0, total_progress_units),
        Some(format!(
            "pricing {task_count} valuation times on {}",
            format_cpu_count(worker_count)
        )),
        ObservedWorkItemUpdate::planned(
            surface_worker_labels(worker_count),
            surface_worker_columns(),
        ),
    )
}

pub(super) fn emit_surface_interval_planning(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    task_count: usize,
) -> Result<(), WorkflowError> {
    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        ProgressMeasure::Unbounded,
        Some(format!(
            "planning pricing intervals for {task_count} valuation times"
        )),
        ObservedWorkItemUpdate::none(),
    )
}

pub(super) fn emit_surface_worker_started(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    input: SurfaceWorkerProgressInput,
) -> Result<(), WorkflowError> {
    let worker_state = SurfaceWorkerState::started(
        input.task_position,
        input.valuation_time_utc_nanos,
        input.node_count,
        input.total_interval_count,
    );

    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        surface_progress_measure(input.completed_progress_units, input.total_progress_units),
        Some(format!(
            "pricing task {}/{} with {} intervals on {}",
            input.task_position + 1,
            input.task_count,
            input.total_interval_count,
            format_cpu_count(input.worker_count)
        )),
        ObservedWorkItemUpdate::active(
            worker_label(input.worker_index),
            0,
            surface_worker_columns(),
            surface_worker_metrics(input.task_count, &worker_state),
        ),
    )
}

pub(super) fn emit_surface_worker_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    input: SurfaceWorkerPricingProgressInput,
) -> Result<(), WorkflowError> {
    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        surface_progress_measure(input.completed_progress_units, input.total_progress_units),
        Some(format!(
            "pricing intervals on {}",
            format_cpu_count(input.worker_count)
        )),
        ObservedWorkItemUpdate::active(
            worker_label(input.worker_index),
            input.worker_state.progress_percent,
            surface_worker_columns(),
            surface_worker_metrics(input.task_count, &input.worker_state),
        ),
    )
}

pub(super) fn emit_surface_worker_completed(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    input: SurfaceWorkerCompletedInput,
) -> Result<(), WorkflowError> {
    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        surface_progress_measure(input.completed_progress_units, input.total_progress_units),
        Some(format!(
            "priced {}/{} valuation times on {}",
            input.completed_task_count,
            input.task_count,
            format_cpu_count(input.worker_count)
        )),
        ObservedWorkItemUpdate::active(
            worker_label(input.worker_index),
            100,
            surface_worker_columns(),
            surface_worker_metrics(input.task_count, &input.worker_state),
        ),
    )
}

fn surface_worker_columns() -> Vec<ProgressWorkItemColumn> {
    vec![
        ProgressWorkItemColumn::new("task", "Task"),
        ProgressWorkItemColumn::new("time", "Time"),
        ProgressWorkItemColumn::new("nodes", "Nodes"),
        ProgressWorkItemColumn::new("interval", "Interval"),
        ProgressWorkItemColumn::new("rows", "Rows"),
        ProgressWorkItemColumn::new("phase", "Phase"),
    ]
}

fn surface_worker_metrics(
    task_count: usize,
    worker_state: &SurfaceWorkerState,
) -> Vec<ProgressWorkItemMetric> {
    vec![
        ProgressWorkItemMetric::new(
            "task",
            format!("{}/{}", worker_state.task_position + 1, task_count),
        ),
        ProgressWorkItemMetric::new(
            "time",
            DateTime::<Utc>::from_timestamp_nanos(worker_state.valuation_time_utc_nanos)
                .to_rfc3339(),
        ),
        ProgressWorkItemMetric::new("nodes", worker_state.node_count.to_string()),
        ProgressWorkItemMetric::new("interval", worker_state.interval_units.clone()),
        ProgressWorkItemMetric::new("rows", worker_state.row_units.clone()),
        ProgressWorkItemMetric::new("phase", worker_state.phase.clone()),
    ]
}

pub(super) fn surface_worker_labels(worker_count: usize) -> Vec<String> {
    (0..worker_count).map(worker_label).collect()
}

pub(super) fn surface_task_progress_totals(task_interval_counts: &[usize]) -> Vec<usize> {
    task_interval_counts
        .iter()
        .copied()
        .map(surface_task_progress_total)
        .collect()
}

const fn surface_task_progress_total(total_intervals: usize) -> usize {
    total_intervals.saturating_add(SURFACE_TASK_FINALIZATION_UNIT)
}

fn surface_progress_measure(completed_units: usize, total_units: usize) -> ProgressMeasure {
    ProgressMeasure::percent(progress_percent(completed_units, total_units))
}

fn worker_label(worker_index: usize) -> String {
    format!("worker {:02}", worker_index + 1)
}

fn surface_worker_event_completed_intervals(event: &PricingProgressEvent) -> usize {
    match event.phase {
        PricingProgressPhase::IntervalProgress => event.completed_units,
        PricingProgressPhase::TransitionRowProgress => event
            .current_interval_index
            .zip(event.total_intervals)
            .map(|(interval_index, total_intervals)| interval_index.min(total_intervals))
            .unwrap_or_default(),
        PricingProgressPhase::StrikePricingProgress
        | PricingProgressPhase::GroupProgress
        | PricingProgressPhase::MaturityProgress => 0,
    }
}

fn surface_worker_interval_units(event: &PricingProgressEvent) -> String {
    match event.phase {
        PricingProgressPhase::IntervalProgress => {
            progress_count_label(Some(event.completed_units), event.total_intervals)
                .unwrap_or_else(|| "-".to_string())
        }
        PricingProgressPhase::TransitionRowProgress => {
            progress_position_label(event.current_interval_index, event.total_intervals)
                .unwrap_or_else(|| "-".to_string())
        }
        PricingProgressPhase::StrikePricingProgress
        | PricingProgressPhase::GroupProgress
        | PricingProgressPhase::MaturityProgress => "-".to_string(),
    }
}

fn surface_worker_row_units(event: &PricingProgressEvent) -> String {
    match event.phase {
        PricingProgressPhase::TransitionRowProgress => pricing_event_units(event),
        _ => "-".to_string(),
    }
}
