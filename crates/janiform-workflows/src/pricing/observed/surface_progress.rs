use chrono::{DateTime, Utc};
use janiform::pricing::{PricingProgressEvent, PricingProgressPhase, pricing_progress_phase_label};
use janiform_terminal_ui::{
    ProgressAdapter, ProgressMeasure, ProgressWorkItemColumn, ProgressWorkItemMetric,
};

use crate::error::WorkflowError;
use crate::progress::{
    bounded_progress_measure, format_cpu_count, progress_count_label, progress_percent,
    progress_scaled_units,
};

use super::super::validation::pricing_validation_error;
use super::progress::{ObservedWorkItemUpdate, emit_observed_stage_progress};
use super::types::{
    ObservedOptionsSnapshotResult, PRICE_VALUATION_SURFACE_STAGE, SURFACE_TASK_FINALIZATION_UNIT,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SurfaceTaskProgressPlan {
    anchor_materialization_interval_units: usize,
    interval_operator_units: usize,
    maturity_operator_application_units: usize,
    strike_row_units: usize,
}

impl SurfaceTaskProgressPlan {
    pub(super) const fn new(
        pricing_interval_units: usize,
        maturity_operator_application_units: usize,
        strike_row_units: usize,
    ) -> Self {
        Self {
            anchor_materialization_interval_units: pricing_interval_units,
            interval_operator_units: pricing_interval_units,
            maturity_operator_application_units,
            strike_row_units,
        }
    }

    const fn pricing_interval_units(self) -> usize {
        self.interval_operator_units
    }

    const fn interval_progress_units(self) -> usize {
        self.anchor_materialization_interval_units
            .saturating_add(self.interval_operator_units)
    }

    const fn maturity_operator_application_units(self) -> usize {
        self.maturity_operator_application_units
    }

    const fn strike_row_units(self) -> usize {
        self.strike_row_units
    }

    fn interval_pass_count(self) -> usize {
        usize::from(self.anchor_materialization_interval_units > 0)
            + usize::from(self.interval_operator_units > 0)
    }

    fn pricing_pass_count(self) -> usize {
        self.interval_pass_count() + usize::from(self.maturity_operator_application_units > 0)
    }

    const fn total_units(self) -> usize {
        self.anchor_materialization_interval_units
            .saturating_add(self.interval_operator_units)
            .saturating_add(self.maturity_operator_application_units)
            .saturating_add(self.strike_row_units)
            .saturating_add(SURFACE_TASK_FINALIZATION_UNIT)
    }
}

pub(super) enum SurfacePricingMessage {
    Started {
        worker_index: usize,
        task_position: usize,
        valuation_time_utc_nanos: i64,
        node_count: usize,
        progress_plan: SurfaceTaskProgressPlan,
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
    progress_plan: SurfaceTaskProgressPlan,
    completed_pricing_units: usize,
    completed_progress_units: usize,
    total_progress_units: usize,
    pub(super) progress_percent: usize,
    pub(super) pass_units: String,
    pub(super) sweep_units: String,
    pub(super) interval_units: String,
    pub(super) phase: String,
    completed_interval_pass_units: usize,
    completed_interval_pass_count: usize,
    completed_interval_sweep_count: usize,
    current_interval_pass_completed_units: usize,
    current_interval_pass_total_units: usize,
    has_interval_pass_progress: bool,
    completed_pricing_pass_count: usize,
}

impl SurfaceWorkerState {
    pub(super) fn started(
        task_position: usize,
        valuation_time_utc_nanos: i64,
        node_count: usize,
        progress_plan: SurfaceTaskProgressPlan,
    ) -> Self {
        Self {
            task_position,
            valuation_time_utc_nanos,
            node_count,
            progress_plan,
            completed_pricing_units: 0,
            completed_progress_units: 0,
            total_progress_units: progress_plan.total_units(),
            progress_percent: 0,
            pass_units: "-".to_string(),
            sweep_units: "-".to_string(),
            interval_units: "-".to_string(),
            phase: "Starting".to_string(),
            completed_interval_pass_units: 0,
            completed_interval_pass_count: 0,
            completed_interval_sweep_count: 0,
            current_interval_pass_completed_units: 0,
            current_interval_pass_total_units: 0,
            has_interval_pass_progress: false,
            completed_pricing_pass_count: 0,
        }
    }

    pub(super) fn apply_pricing_event(&mut self, event: &PricingProgressEvent) {
        let progress_update = self.progress_update_for_event(event);

        self.completed_pricing_units = self.completed_pricing_units.max(progress_update.units);
        self.completed_progress_units = self.completed_pricing_units.min(self.total_progress_units);

        self.progress_percent =
            progress_percent(self.completed_progress_units, self.total_progress_units);

        self.completed_pricing_pass_count = self
            .completed_pricing_pass_count
            .max(progress_update.completed_pass_count)
            .min(self.progress_plan.pricing_pass_count());

        self.pass_units = surface_worker_pass_units(
            self.completed_pricing_pass_count,
            self.progress_plan.pricing_pass_count(),
        );

        self.interval_units = surface_worker_interval_units(event);

        let current_sweep_number = self
            .has_interval_pass_progress
            .then_some(self.completed_interval_sweep_count.saturating_add(1));

        self.sweep_units = surface_worker_sweep_units(event.phase, current_sweep_number);
        self.phase = pricing_progress_phase_label(event.phase).to_string();
    }

    pub(super) fn complete(&mut self) {
        self.completed_progress_units = self.total_progress_units;
        self.completed_pricing_units = self.total_progress_units;

        self.progress_percent = 100;

        self.completed_pricing_pass_count = self.progress_plan.pricing_pass_count();

        self.pass_units = surface_worker_pass_units(
            self.completed_pricing_pass_count,
            self.progress_plan.pricing_pass_count(),
        );

        self.interval_units = "-".to_string();
        self.sweep_units = "-".to_string();
        self.phase = "done".to_string();
    }

    fn progress_update_for_event(&mut self, event: &PricingProgressEvent) -> SurfaceProgressUpdate {
        match event.phase {
            PricingProgressPhase::IntervalProgress => self.interval_progress_update(event),
            PricingProgressPhase::TransitionRowProgress => {
                self.transition_row_progress_update(event)
            }
            PricingProgressPhase::MaturityProgress => {
                let maturity_units = progress_scaled_units(
                    event.completed_units,
                    event.total_units,
                    self.progress_plan.maturity_operator_application_units(),
                );

                let completed_pass_count =
                    self.progress_plan
                        .interval_pass_count()
                        .saturating_add(usize::from(
                            event.total_units > 0 && event.completed_units >= event.total_units,
                        ));

                SurfaceProgressUpdate {
                    units: self
                        .progress_plan
                        .interval_progress_units()
                        .saturating_add(maturity_units),
                    completed_pass_count,
                }
            }
            PricingProgressPhase::StrikePricingProgress => SurfaceProgressUpdate {
                units: self
                    .progress_plan
                    .interval_progress_units()
                    .saturating_add(self.progress_plan.maturity_operator_application_units())
                    .saturating_add(surface_worker_strike_progress_units(
                        event,
                        self.progress_plan,
                    )),
                completed_pass_count: self.progress_plan.pricing_pass_count(),
            },
            PricingProgressPhase::GroupProgress => SurfaceProgressUpdate {
                units: self
                    .progress_plan
                    .interval_progress_units()
                    .saturating_add(self.progress_plan.maturity_operator_application_units())
                    .saturating_add(surface_worker_group_progress_units(
                        event,
                        self.progress_plan,
                    )),
                completed_pass_count: self.progress_plan.pricing_pass_count(),
            },
        }
    }

    fn interval_progress_update(&mut self, event: &PricingProgressEvent) -> SurfaceProgressUpdate {
        let completed_units = event.completed_units.min(event.total_units);

        self.record_interval_pass_progress(completed_units, event.total_units);

        let current_pass_is_complete =
            event.total_units > 0 && completed_units >= event.total_units;

        let completed_pass_count = self
            .completed_interval_pass_count
            .saturating_add(usize::from(current_pass_is_complete))
            .min(self.progress_plan.interval_pass_count());

        SurfaceProgressUpdate {
            units: self
                .completed_interval_pass_units
                .saturating_add(completed_units)
                .min(self.progress_plan.interval_progress_units()),
            completed_pass_count,
        }
    }

    fn transition_row_progress_update(
        &mut self,
        event: &PricingProgressEvent,
    ) -> SurfaceProgressUpdate {
        let total_intervals = event
            .total_intervals
            .unwrap_or(self.progress_plan.pricing_interval_units());

        let completed_units = surface_worker_transition_row_progress_units(event, total_intervals);

        self.record_interval_pass_progress(completed_units, total_intervals);

        let current_pass_is_complete = total_intervals > 0 && completed_units >= total_intervals;

        let completed_pass_count = self
            .completed_interval_pass_count
            .saturating_add(usize::from(current_pass_is_complete))
            .min(self.progress_plan.interval_pass_count());

        SurfaceProgressUpdate {
            units: self
                .completed_interval_pass_units
                .saturating_add(completed_units)
                .min(self.progress_plan.interval_progress_units()),
            completed_pass_count,
        }
    }

    fn record_interval_pass_progress(&mut self, completed_units: usize, total_units: usize) {
        let interval_progress_reset = self.has_interval_pass_progress
            && completed_units < self.current_interval_pass_completed_units;

        if interval_progress_reset {
            self.completed_interval_sweep_count =
                self.completed_interval_sweep_count.saturating_add(1);

            self.completed_interval_pass_units = self
                .completed_interval_pass_units
                .saturating_add(self.current_interval_pass_total_units)
                .min(self.progress_plan.interval_progress_units());

            self.completed_interval_pass_count = self
                .completed_interval_pass_count
                .saturating_add(1)
                .min(self.progress_plan.interval_pass_count());
        }

        self.current_interval_pass_completed_units = completed_units;
        self.current_interval_pass_total_units = total_units;

        self.has_interval_pass_progress = true;
    }
}

struct SurfaceProgressUpdate {
    units: usize,
    completed_pass_count: usize,
}

pub(super) struct SurfaceWorkerProgressInput {
    pub(super) worker_index: usize,
    pub(super) task_position: usize,
    pub(super) task_count: usize,
    pub(super) completed_task_count: usize,
    pub(super) worker_count: usize,
    pub(super) valuation_time_utc_nanos: i64,
    pub(super) node_count: usize,
    pub(super) progress_plan: SurfaceTaskProgressPlan,
}

pub(super) struct SurfaceWorkerPricingProgressInput {
    pub(super) worker_index: usize,
    pub(super) task_count: usize,
    pub(super) completed_task_count: usize,
    pub(super) worker_count: usize,
    pub(super) worker_state: SurfaceWorkerState,
}

pub(super) struct SurfaceWorkerCompletedInput {
    pub(super) worker_index: usize,
    pub(super) task_count: usize,
    pub(super) completed_task_count: usize,
    pub(super) worker_count: usize,
    pub(super) worker_state: SurfaceWorkerState,
}

pub(super) struct SurfacePricingResultCollector<'collector, 'adapter> {
    progress_adapter: &'collector mut Option<&'adapter mut ProgressAdapter>,
    worker_states: Vec<Option<SurfaceWorkerState>>,
    completed_task_count: usize,
    results: Vec<Option<ObservedOptionsSnapshotResult>>,
    task_count: usize,
    worker_count: usize,
}

impl<'collector, 'adapter> SurfacePricingResultCollector<'collector, 'adapter> {
    pub(super) fn new(
        task_count: usize,
        worker_count: usize,
        progress_adapter: &'collector mut Option<&'adapter mut ProgressAdapter>,
    ) -> Self {
        Self {
            progress_adapter,
            worker_states: (0..worker_count).map(|_| None).collect(),
            completed_task_count: 0,
            results: (0..task_count).map(|_| None).collect(),
            task_count,
            worker_count,
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
                progress_plan,
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
                    progress_plan,
                ));

                emit_surface_worker_started(
                    self.progress_adapter,
                    SurfaceWorkerProgressInput {
                        worker_index,
                        task_position,
                        task_count: self.task_count,
                        completed_task_count: self.completed_task_count,
                        worker_count: self.worker_count,
                        valuation_time_utc_nanos,
                        node_count,
                        progress_plan,
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

                emit_surface_worker_progress(
                    self.progress_adapter,
                    SurfaceWorkerPricingProgressInput {
                        worker_index,
                        task_count: self.task_count,
                        completed_task_count: self.completed_task_count,
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
) -> Result<(), WorkflowError> {
    emit_observed_stage_progress(
        progress_adapter,
        PRICE_VALUATION_SURFACE_STAGE,
        surface_task_progress_measure(0, task_count)?,
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
        PRICE_VALUATION_SURFACE_STAGE,
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
        input.progress_plan,
    );

    emit_observed_stage_progress(
        progress_adapter,
        PRICE_VALUATION_SURFACE_STAGE,
        surface_task_progress_measure(input.completed_task_count, input.task_count)?,
        Some(format!(
            "pricing task {}/{} with {} intervals on {}",
            input.task_position + 1,
            input.task_count,
            input.progress_plan.pricing_interval_units(),
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
        PRICE_VALUATION_SURFACE_STAGE,
        surface_task_progress_measure(input.completed_task_count, input.task_count)?,
        Some(format!(
            "{} on {}",
            input.worker_state.phase,
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
        PRICE_VALUATION_SURFACE_STAGE,
        surface_task_progress_measure(input.completed_task_count, input.task_count)?,
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
        ProgressWorkItemColumn::new("pass", "Pass"),
        ProgressWorkItemColumn::new("sweep", "Sweep"),
        ProgressWorkItemColumn::new("interval", "Interval"),
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
        ProgressWorkItemMetric::new("pass", worker_state.pass_units.clone()),
        ProgressWorkItemMetric::new("sweep", worker_state.sweep_units.clone()),
        ProgressWorkItemMetric::new("interval", worker_state.interval_units.clone()),
        ProgressWorkItemMetric::new("phase", worker_state.phase.clone()),
    ]
}

pub(super) fn surface_worker_labels(worker_count: usize) -> Vec<String> {
    (0..worker_count).map(worker_label).collect()
}

fn surface_task_progress_measure(
    completed_task_count: usize,
    task_count: usize,
) -> Result<ProgressMeasure, WorkflowError> {
    bounded_progress_measure(completed_task_count.min(task_count), task_count)
}

fn worker_label(worker_index: usize) -> String {
    format!("worker {:02}", worker_index + 1)
}

fn surface_worker_pass_units(completed_pass_count: usize, total_pass_count: usize) -> String {
    if total_pass_count == 0 {
        return "-".to_string();
    }

    progress_count_label(
        Some(completed_pass_count.min(total_pass_count)),
        Some(total_pass_count),
    )
    .map(|label| format!("{label} done"))
    .unwrap_or_else(|| "-".to_string())
}

fn surface_worker_sweep_units(
    phase: PricingProgressPhase,
    current_sweep_number: Option<usize>,
) -> String {
    if matches!(
        phase,
        PricingProgressPhase::IntervalProgress | PricingProgressPhase::TransitionRowProgress
    ) {
        return current_sweep_number
            .map(|sweep_number| sweep_number.to_string())
            .unwrap_or_else(|| "-".to_string());
    }

    "-".to_string()
}

fn surface_worker_interval_units(event: &PricingProgressEvent) -> String {
    match event.phase {
        PricingProgressPhase::IntervalProgress => progress_count_label(
            Some(event.completed_units.min(event.total_units)),
            Some(event.total_units),
        )
        .unwrap_or_else(|| "-".to_string()),
        PricingProgressPhase::TransitionRowProgress => {
            surface_worker_transition_interval_units(event)
        }
        PricingProgressPhase::GroupProgress
        | PricingProgressPhase::MaturityProgress
        | PricingProgressPhase::StrikePricingProgress => "-".to_string(),
    }
}

fn surface_worker_transition_interval_units(event: &PricingProgressEvent) -> String {
    let Some(total_intervals) = event.total_intervals else {
        return "-".to_string();
    };

    progress_count_label(
        Some(surface_worker_transition_row_progress_units(
            event,
            total_intervals,
        )),
        Some(total_intervals),
    )
    .unwrap_or_else(|| "-".to_string())
}

fn surface_worker_transition_row_progress_units(
    event: &PricingProgressEvent,
    interval_operator_units: usize,
) -> usize {
    let completed_interval_count = event
        .current_interval_index
        .unwrap_or(interval_operator_units)
        .min(interval_operator_units);

    let current_interval_completed =
        event.total_units > 0 && event.completed_units >= event.total_units;

    completed_interval_count
        .saturating_add(usize::from(current_interval_completed))
        .min(interval_operator_units)
}

fn surface_worker_strike_progress_units(
    event: &PricingProgressEvent,
    progress_plan: SurfaceTaskProgressPlan,
) -> usize {
    let total_maturities = event
        .total_maturities
        .unwrap_or(progress_plan.strike_row_units())
        .max(1);

    let maturity_index = event
        .current_maturity_index
        .unwrap_or_default()
        .min(total_maturities);

    let previous_row_units = progress_scaled_units(
        maturity_index,
        total_maturities,
        progress_plan.strike_row_units(),
    );

    let next_row_units = progress_scaled_units(
        maturity_index.saturating_add(1),
        total_maturities,
        progress_plan.strike_row_units(),
    );

    let current_row_units = next_row_units.saturating_sub(previous_row_units);

    previous_row_units.saturating_add(progress_scaled_units(
        event.completed_units,
        event.total_units,
        current_row_units,
    ))
}

fn surface_worker_group_progress_units(
    event: &PricingProgressEvent,
    progress_plan: SurfaceTaskProgressPlan,
) -> usize {
    progress_scaled_units(
        event.completed_units,
        event.total_units,
        progress_plan.strike_row_units(),
    )
}
