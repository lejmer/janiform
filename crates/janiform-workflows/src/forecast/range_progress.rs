use std::ops::Range;

use janiform_work_scheduler::StreamingWorkProgress;

use crate::error::WorkflowError;
use crate::progress::{format_cpu_count, progress_percent};

use super::progress::{
    ForecastProgressEvent, ForecastProgressPhase, ForecastProgressWorkItemColumn,
    ForecastProgressWorkItemMetric,
};
use super::request::validation_error;

pub(super) enum ForecastRangeProgress {
    Started,
    Advanced { completed_units: usize },
}

pub(super) struct ForecastRangeCollector<'observer, Output> {
    phase: ForecastProgressPhase,
    ranges: Vec<Range<usize>>,
    range_completed_units: Vec<usize>,
    completed_units: usize,
    total_units: usize,
    worker_count: usize,
    unit_key: &'static str,
    unit_label: &'static str,
    outputs: Vec<Output>,
    emit_progress: &'observer mut dyn FnMut(ForecastProgressEvent),
}

impl<'observer, Output> ForecastRangeCollector<'observer, Output> {
    pub(super) fn new(
        phase: ForecastProgressPhase,
        ranges: Vec<Range<usize>>,
        worker_count: usize,
        unit_key: &'static str,
        unit_label: &'static str,
        emit_progress: &'observer mut dyn FnMut(ForecastProgressEvent),
    ) -> Self {
        let total_units = ranges.iter().map(Range::len).sum();

        Self {
            phase,
            range_completed_units: (0..ranges.len()).map(|_| 0).collect(),
            ranges,
            completed_units: 0,
            total_units,
            worker_count,
            unit_key,
            unit_label,
            outputs: Vec::new(),
            emit_progress,
        }
    }

    pub(super) fn emit_plan(&mut self, signal: &str) {
        let latest_signal = format!("{signal} on {}", format_cpu_count(self.worker_count));

        let planned_work_item_labels = self
            .ranges
            .iter()
            .map(|range| self.range_label(range))
            .collect();

        let work_item_columns = self.work_item_columns();

        let event = ForecastProgressEvent::new(self.phase, self.completed_units, self.total_units)
            .with_latest_signal(latest_signal)
            .with_work_plan(planned_work_item_labels, work_item_columns);

        (self.emit_progress)(event);
    }

    pub(super) fn record_progress(
        &mut self,
        progress: StreamingWorkProgress<ForecastRangeProgress>,
    ) -> Result<(), WorkflowError> {
        let Some(range) = self.ranges.get(progress.job_position).cloned() else {
            return Err(validation_error(
                "forecast_progress",
                "worker progress referenced an unknown range",
            ));
        };

        match progress.progress {
            ForecastRangeProgress::Started => {
                self.emit_range_progress(&range, 0);
            }
            ForecastRangeProgress::Advanced { completed_units } => {
                self.record_range_units(progress.job_position, completed_units)?;

                self.emit_range_progress(&range, completed_units);
            }
        }

        Ok(())
    }

    pub(super) fn record_output(
        &mut self,
        job_position: usize,
        range: &Range<usize>,
    ) -> Result<(), WorkflowError> {
        self.record_range_units(job_position, range.len())?;

        let range_total_units = range.len();

        self.emit_range_progress(range, range_total_units);

        Ok(())
    }

    pub(super) fn push_output(&mut self, output: Output) {
        self.outputs.push(output);
    }

    pub(super) fn into_outputs(self) -> Vec<Output> {
        self.outputs
    }

    fn record_range_units(
        &mut self,
        job_position: usize,
        completed_units: usize,
    ) -> Result<(), WorkflowError> {
        let Some(range) = self.ranges.get(job_position) else {
            return Err(validation_error(
                "forecast_progress",
                "worker progress referenced an unknown range",
            ));
        };

        let Some(range_completed_units) = self.range_completed_units.get_mut(job_position) else {
            return Err(validation_error(
                "forecast_progress",
                "worker progress referenced an unknown range",
            ));
        };

        let bounded_completed_units = completed_units.min(range.len());
        let delta = bounded_completed_units.saturating_sub(*range_completed_units);

        *range_completed_units = bounded_completed_units;

        self.completed_units = self.completed_units.saturating_add(delta);

        Ok(())
    }

    fn emit_range_progress(&mut self, range: &Range<usize>, range_completed_units: usize) {
        let range_total_units = range.len();
        let bounded_range_completed_units = range_completed_units.min(range_total_units);

        let latest_signal = format!(
            "{} {}/{} on {}",
            self.unit_label.to_lowercase(),
            self.completed_units,
            self.total_units,
            format_cpu_count(self.worker_count)
        );

        let work_item_label = self.range_label(range);

        let work_item_progress_percent =
            progress_percent(bounded_range_completed_units, range_total_units);

        let work_item_metrics = vec![
            ForecastProgressWorkItemMetric::new(
                self.unit_key,
                format!("{bounded_range_completed_units}/{range_total_units}"),
            ),
            ForecastProgressWorkItemMetric::new("cpu", self.worker_count.to_string()),
        ];

        let work_item_columns = self.work_item_columns();

        let event = ForecastProgressEvent::new(self.phase, self.completed_units, self.total_units)
            .with_latest_signal(latest_signal)
            .with_work_item(
                work_item_label,
                work_item_progress_percent,
                work_item_metrics,
            )
            .with_work_plan(Vec::new(), work_item_columns);

        (self.emit_progress)(event);
    }

    fn work_item_columns(&self) -> Vec<ForecastProgressWorkItemColumn> {
        vec![
            ForecastProgressWorkItemColumn::new(self.unit_key, self.unit_label),
            ForecastProgressWorkItemColumn::new("cpu", "CPU"),
        ]
    }

    fn range_label(&self, range: &Range<usize>) -> String {
        if range.is_empty() {
            return format!("{} empty", self.unit_key);
        }

        let start = match self.unit_key {
            "intervals" => range.start + 1,
            _ => range.start,
        };

        let end = match self.unit_key {
            "intervals" => range.end,
            _ => range.end.saturating_sub(1),
        };

        if start == end {
            format!("{} {start}", self.unit_key)
        } else {
            format!("{} {start}-{end}", self.unit_key)
        }
    }
}
