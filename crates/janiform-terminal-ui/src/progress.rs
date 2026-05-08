use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use console::Term;
use indicatif::ProgressBar;
use indicatif::ProgressDrawTarget;
use indicatif::ProgressStyle;

mod contract;
mod display;
mod refresh;
mod render;
mod table;

use display::{
    format_elapsed_duration, format_elapsed_seconds, progress_measure_label,
    progress_percent_label, work_item_status_text, work_item_waiting_status_text,
};
use refresh::MessageRefresh;
use render::ProgressRenderState;

pub use contract::{
    ProgressContractError, ProgressEvent, ProgressMeasure, ProgressSnapshot, ProgressStageSnapshot,
    ProgressStageStatus, ProgressWorkItemColumn, ProgressWorkItemMetric, ProgressWorkItemSnapshot,
};

const BOUNDED_PROGRESS_TEMPLATE: &str = "\n{spinner:.cyan} {msg}";
const SPINNER_TICK_INTERVAL: Duration = Duration::from_millis(100);
const UNBOUNDED_PROGRESS_TEMPLATE: &str = "\n{spinner:.cyan} {msg}";

/// Progress adapter over semantic progress events.
pub struct ProgressAdapter {
    snapshot: ProgressSnapshot,
    run_started_at: Option<Instant>,
    active_stage_started_at: Option<Instant>,
    active_work_started_at_by_label: BTreeMap<String, Instant>,
    progress_bar: ProgressBar,
    render_state: Arc<Mutex<ProgressRenderState>>,
    message_refresh: MessageRefresh,
    steady_tick_enabled: bool,
    dirty: bool,
    color_enabled: bool,
}

impl ProgressAdapter {
    /// Create an adapter that writes live progress through the provided draw target.
    pub fn new(draw_target: ProgressDrawTarget) -> Self {
        let progress_bar = ProgressBar::with_draw_target(None, draw_target);
        if let Ok(style) = ProgressStyle::with_template(UNBOUNDED_PROGRESS_TEMPLATE) {
            progress_bar.set_style(style);
        }

        let color_enabled =
            console::colors_enabled_stderr() && std::env::var_os("NO_COLOR").is_none();

        let render_state = Arc::new(Mutex::new(ProgressRenderState {
            color_enabled,
            ..ProgressRenderState::default()
        }));

        Self {
            snapshot: ProgressSnapshot::default(),
            run_started_at: None,
            active_stage_started_at: None,
            active_work_started_at_by_label: BTreeMap::new(),
            progress_bar,
            render_state,
            message_refresh: MessageRefresh::enabled(),
            steady_tick_enabled: false,
            dirty: false,
            color_enabled,
        }
    }

    /// Create a hidden adapter for tests and machine output modes.
    pub fn hidden() -> Self {
        let mut adapter = Self::new(ProgressDrawTarget::hidden());
        adapter.message_refresh.disable();

        adapter
    }

    /// Create an adapter that writes live progress to stderr.
    pub fn stderr() -> Self {
        Self::new(ProgressDrawTarget::stderr())
    }

    /// Consume a structured event without drawing immediately.
    ///
    /// Multiple high-frequency calls coalesce into one visible update when
    /// `draw_latest` is called.
    pub fn observe(&mut self, event: ProgressEvent) {
        match event {
            ProgressEvent::RunStarted {
                run_id,
                scope,
                input_count,
                output_path,
                stage_sequence,
                completed_stage_count,
            } => {
                self.snapshot.run_id = Some(run_id);
                self.snapshot.scope = Some(scope);
                self.snapshot.input_count = Some(input_count);
                self.snapshot.output_path = output_path;

                self.snapshot.stages = stage_sequence
                    .into_iter()
                    .enumerate()
                    .map(|(stage_index, name)| ProgressStageSnapshot {
                        name,
                        stage_number: stage_index + 1,
                        status: if stage_index < completed_stage_count {
                            ProgressStageStatus::Completed
                        } else {
                            ProgressStageStatus::Pending
                        },
                        elapsed: None,
                    })
                    .collect();

                self.run_started_at = Some(Instant::now());

                self.snapshot.terminal_status = None;
                self.snapshot.failure_reason = None;
            }
            ProgressEvent::StageStarted {
                stage,
                stage_number,
                stage_count,
            } => {
                self.snapshot.active_stage = Some(stage);
                self.snapshot.stage_number = Some(stage_number);
                self.snapshot.stage_count = Some(stage_count);

                self.active_stage_started_at = Some(Instant::now());

                self.clear_progress_details();
                self.update_stage_status(stage_number, ProgressStageStatus::Running, None);
            }
            ProgressEvent::StageProgress {
                phase,
                measure,
                work_item_label,
                work_item_progress_percent,
                planned_work_item_labels,
                work_item_columns,
                work_item_metrics,
                latest_signal,
            } => {
                if self.snapshot.phase.as_deref() != Some(phase.as_str()) {
                    self.clear_work_item_details();
                }

                self.snapshot.phase = Some(phase.clone());
                self.snapshot.measure = Some(measure);
                self.snapshot.latest_signal = latest_signal.clone();

                self.merge_work_item_columns(work_item_columns);
                self.seed_work_items(planned_work_item_labels);

                if let Some(work_item_label) = work_item_label {
                    self.update_work_item(
                        work_item_label,
                        measure,
                        work_item_progress_percent,
                        work_item_metrics,
                    );
                }
            }
            ProgressEvent::RunProgress {
                phase,
                measure,
                latest_signal,
            } => {
                self.snapshot.active_stage = None;
                self.snapshot.stage_number = None;
                self.snapshot.stage_count = None;
                self.snapshot.phase = Some(phase);
                self.snapshot.measure = Some(measure);
                self.snapshot.latest_signal = latest_signal;
                self.snapshot.terminal_status = None;
                self.snapshot.failure_reason = None;
            }
            ProgressEvent::StageCompleted {
                stage,
                runtime_seconds,
            } => {
                self.snapshot.completed_stage = Some(stage);

                self.clear_progress_details();

                let elapsed = format_elapsed_seconds(runtime_seconds);

                if let Some(stage_number) = self.snapshot.stage_number {
                    self.update_stage_status(
                        stage_number,
                        ProgressStageStatus::Completed,
                        Some(elapsed),
                    );
                }

                self.active_stage_started_at = None;

                self.progress_bar.reset();
                self.progress_bar.unset_length();
            }
            ProgressEvent::RunCompleted { status } => {
                self.snapshot.terminal_status = Some(status);
                self.snapshot.active_stage = None;
                self.snapshot.stage_number = None;
                self.snapshot.stage_count = None;

                self.active_stage_started_at = None;

                self.clear_progress_details();

                self.progress_bar.reset();
                self.progress_bar.unset_length();
            }
            ProgressEvent::RunFailed { reason } => {
                self.snapshot.failure_reason = Some(reason);

                if let Some(stage_number) = self.snapshot.stage_number {
                    self.update_stage_status(
                        stage_number,
                        ProgressStageStatus::Failed,
                        self.active_stage_elapsed_label(),
                    );
                }

                self.progress_bar.reset();
                self.progress_bar.unset_length();
            }
        }

        self.sync_render_state();
        self.dirty = true;
    }

    /// Return the latest semantic progress snapshot.
    pub const fn snapshot(&self) -> &ProgressSnapshot {
        &self.snapshot
    }

    /// Draw the latest coalesced state.
    pub fn draw_latest(&mut self) -> Result<(), ProgressContractError> {
        if !self.dirty {
            return Ok(());
        }

        let measure = self.snapshot.measure.unwrap_or(ProgressMeasure::Unbounded);

        let message = self.progress_message();
        match measure {
            ProgressMeasure::Bounded {
                completed_units,
                total_units,
            } => {
                let total_units = u64::try_from(total_units)?;
                let completed_units = u64::try_from(completed_units)?;

                let progress_bar = self.progress_bar_for_bounded(total_units);

                progress_bar.set_message(message);
                progress_bar.set_position(completed_units);

                self.enable_steady_tick();
                self.start_message_refresh();
            }
            ProgressMeasure::Unbounded => {
                let progress_bar = self.progress_bar_for_unbounded();

                progress_bar.set_message(message);
                progress_bar.tick();

                self.enable_steady_tick();
                self.start_message_refresh();
            }
        }

        self.dirty = false;
        Ok(())
    }

    /// Clear the live progress line before rendering terminal summaries.
    pub fn finish_and_clear(&mut self) {
        self.message_refresh.stop();
        self.progress_bar.finish_and_clear();
    }

    /// Temporarily clear the live progress line while writing other terminal output.
    pub fn suspend_for_output<T>(&self, operation: impl FnOnce() -> T) -> T {
        self.progress_bar.suspend(operation)
    }

    /// Finish the live progress section while leaving the final rows on screen.
    pub fn finish_and_keep(&mut self) -> io::Result<()> {
        self.message_refresh.stop();

        let message = self.progress_message();

        self.progress_bar.finish_and_clear();

        Term::stderr().write_line("")?;
        Term::stderr().write_line(&format!("  {message}"))?;
        Term::stderr().write_line("")
    }

    fn current_render_state(&self) -> ProgressRenderState {
        ProgressRenderState {
            snapshot: self.snapshot.clone(),
            run_started_at: self.run_started_at,
            active_stage_started_at: self.active_stage_started_at,
            active_work_started_at_by_label: self.active_work_started_at_by_label.clone(),
            color_enabled: self.color_enabled,
        }
    }

    fn sync_render_state(&self) {
        let Ok(mut render_state) = self.render_state.lock() else {
            return;
        };

        *render_state = self.current_render_state();
    }

    fn progress_bar_for_bounded(&mut self, total_units: u64) -> ProgressBar {
        self.progress_bar.set_length(total_units);

        if let Ok(style) = ProgressStyle::with_template(BOUNDED_PROGRESS_TEMPLATE) {
            self.progress_bar.set_style(style);
        }

        self.progress_bar.clone()
    }

    fn progress_bar_for_unbounded(&mut self) -> ProgressBar {
        self.progress_bar.unset_length();

        if let Ok(style) = ProgressStyle::with_template(UNBOUNDED_PROGRESS_TEMPLATE) {
            self.progress_bar.set_style(style);
        }

        self.progress_bar.clone()
    }

    fn enable_steady_tick(&mut self) {
        if self.steady_tick_enabled {
            return;
        }

        self.progress_bar.enable_steady_tick(SPINNER_TICK_INTERVAL);
        self.steady_tick_enabled = true;
    }

    fn start_message_refresh(&mut self) {
        self.message_refresh
            .start(self.progress_bar.clone(), Arc::clone(&self.render_state));
    }

    fn progress_message(&self) -> String {
        self.current_render_state().progress_message()
    }

    fn update_stage_status(
        &mut self,
        stage_number: usize,
        status: ProgressStageStatus,
        elapsed: Option<String>,
    ) {
        if let Some(stage) = self
            .snapshot
            .stages
            .iter_mut()
            .find(|stage| stage.stage_number == stage_number)
        {
            stage.status = status;
            if elapsed.is_some() {
                stage.elapsed = elapsed;
            }
        }
    }

    fn active_stage_elapsed_label(&self) -> Option<String> {
        self.active_stage_started_at
            .map(|started_at| format_elapsed_duration(started_at.elapsed()))
    }

    fn clear_progress_details(&mut self) {
        self.snapshot.phase = None;
        self.snapshot.measure = None;
        self.snapshot.latest_signal = None;

        self.clear_work_item_details();
    }

    fn clear_work_item_details(&mut self) {
        self.snapshot.work_items.clear();
        self.snapshot.work_item_columns.clear();
        self.active_work_started_at_by_label.clear();
    }

    fn seed_work_items(&mut self, labels: Vec<String>) {
        if labels.is_empty() {
            return;
        }

        for label in labels {
            if self
                .snapshot
                .work_items
                .iter()
                .any(|work_item| work_item.label == label)
            {
                continue;
            }

            self.snapshot.work_items.push(ProgressWorkItemSnapshot {
                label,
                progress: progress_percent_label(0),
                elapsed: "00:00:00".to_string(),
                status: work_item_waiting_status_text().to_string(),
                metrics: BTreeMap::new(),
            });
        }
    }

    fn update_work_item(
        &mut self,
        label: String,
        measure: ProgressMeasure,
        progress_percent: Option<usize>,
        metrics: Vec<ProgressWorkItemMetric>,
    ) {
        let status = work_item_status_text(progress_percent).to_string();

        let started_at = self
            .active_work_started_at_by_label
            .entry(label.clone())
            .or_insert_with(Instant::now);

        let elapsed = format_elapsed_duration(started_at.elapsed());

        let progress = progress_percent
            .map(progress_percent_label)
            .unwrap_or_else(|| progress_measure_label(measure));

        let metrics = metrics
            .into_iter()
            .map(|metric| (metric.key, metric.value))
            .collect::<BTreeMap<_, _>>();

        if let Some(work_item) = self
            .snapshot
            .work_items
            .iter_mut()
            .find(|work_item| work_item.label == label)
        {
            work_item.progress = progress;
            work_item.elapsed = elapsed;
            work_item.status = status;
            work_item.metrics.extend(metrics);

            return;
        }

        self.snapshot.work_items.push(ProgressWorkItemSnapshot {
            label,
            progress,
            elapsed,
            status,
            metrics,
        });
    }

    fn merge_work_item_columns(&mut self, columns: Vec<ProgressWorkItemColumn>) {
        for column in columns {
            if self
                .snapshot
                .work_item_columns
                .iter()
                .any(|existing_column| existing_column.key == column.key)
            {
                continue;
            }

            self.snapshot.work_item_columns.push(column);
        }
    }
}
