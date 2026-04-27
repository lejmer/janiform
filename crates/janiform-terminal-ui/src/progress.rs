use std::collections::BTreeMap;
use std::io;
use std::num::TryFromIntError;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use console::Term;
use console::measure_text_width;
use indicatif::ProgressBar;
use indicatif::ProgressDrawTarget;
use indicatif::ProgressStyle;
use thiserror::Error;

use crate::style::SemanticStyle;
use crate::style::style_contract;

mod display;

use display::{
    format_elapsed_duration, format_elapsed_seconds, progress_bar_label, progress_measure_label,
    progress_percent, progress_percent_label, stage_status_marker, stage_status_text,
    truncate_for_table, work_item_status_is_completed, work_item_status_sort_rank,
    work_item_status_text, work_item_waiting_status_text,
};

const BOUNDED_PROGRESS_TEMPLATE: &str = "\n{spinner:.cyan} {msg}";
const CONTEXT_WORK_ITEM_COLUMN_MAX_WIDTH: usize = 18;
const CONTEXT_WORK_ITEM_COLUMN_MIN_WIDTH: usize = 8;
const WORK_ITEM_ELAPSED_COLUMN_WIDTH: usize = 10;
const WORK_ITEM_LABEL_COLUMN_WIDTH: usize = 34;
const WORK_ITEM_PROGRESS_COLUMN_WIDTH: usize = 26;
const WORK_ITEM_STATUS_COLUMN_WIDTH: usize = 10;
const MESSAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const SPINNER_TICK_INTERVAL: Duration = Duration::from_millis(100);
const UNBOUNDED_PROGRESS_TEMPLATE: &str = "\n{spinner:.cyan} {msg}";

#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkItemTableColumn {
    key: String,
    label: String,
    width: usize,
}

#[derive(Clone, Debug, Default)]
struct ProgressRenderState {
    snapshot: ProgressSnapshot,
    run_started_at: Option<Instant>,
    active_stage_started_at: Option<Instant>,
    active_work_started_at_by_label: BTreeMap<String, Instant>,
    color_enabled: bool,
}

impl WorkItemTableColumn {
    fn new(key: impl Into<String>, label: impl Into<String>, width: usize) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            width,
        }
    }
}

/// Error returned when a structured progress event violates the UI contract.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum ProgressContractError {
    #[error("bounded progress total must be greater than zero")]
    EmptyBoundedTotal,
    #[error("bounded progress completed units must not exceed total units")]
    CompletedUnitsExceedTotal,
    #[error("progress units do not fit the terminal progress backend")]
    ProgressUnitsTooLarge,
}

impl From<TryFromIntError> for ProgressContractError {
    fn from(_: TryFromIntError) -> Self {
        Self::ProgressUnitsTooLarge
    }
}

/// Progress units supplied by the runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgressMeasure {
    /// Bounded progress backed by real runtime totals.
    Bounded {
        /// Completed units.
        completed_units: usize,
        /// Total units.
        total_units: usize,
    },
    /// Unbounded progress where only a spinner can be truthful.
    Unbounded,
}

impl ProgressMeasure {
    /// Create bounded progress from real runtime totals.
    pub fn bounded(
        completed_units: usize,
        total_units: usize,
    ) -> Result<Self, ProgressContractError> {
        if total_units == 0 {
            return Err(ProgressContractError::EmptyBoundedTotal);
        }
        if completed_units > total_units {
            return Err(ProgressContractError::CompletedUnitsExceedTotal);
        }

        Ok(Self::Bounded {
            completed_units,
            total_units,
        })
    }
}

/// Structured progress event consumed by the terminal progress adapter.
#[derive(Clone, Debug, PartialEq)]
pub enum ProgressEvent {
    /// A run has started.
    RunStarted {
        /// Stable run identifier.
        run_id: String,
        /// User-readable run scope.
        scope: String,
        /// Count of input groups or files.
        input_count: usize,
        /// Output path when known.
        output_path: Option<String>,
        /// User-facing ordered stage sequence.
        stage_sequence: Vec<String>,
    },
    /// A stage has started.
    StageStarted {
        /// Stage name.
        stage: String,
        /// One-based stage number.
        stage_number: usize,
        /// Total stage count.
        stage_count: usize,
    },
    /// A stage emitted progress.
    StageProgress {
        /// Current phase or subphase.
        phase: String,
        /// Progress measure.
        measure: ProgressMeasure,
        /// Active work item when the runtime reports concurrent or candidate-level work.
        work_item_label: Option<String>,
        /// Active work item completion percent when known.
        work_item_progress_percent: Option<usize>,
        /// Work items expected to appear in the active phase.
        planned_work_item_labels: Vec<String>,
        /// Context-defined work item table columns.
        work_item_columns: Vec<ProgressWorkItemColumn>,
        /// Context-defined work item table values for the active item.
        work_item_metrics: Vec<ProgressWorkItemMetric>,
        /// Latest meaningful signal.
        latest_signal: Option<String>,
    },
    /// The overall run emitted progress outside a numbered stage.
    RunProgress {
        /// Current phase or subphase.
        phase: String,
        /// Progress measure.
        measure: ProgressMeasure,
        /// Latest meaningful signal.
        latest_signal: Option<String>,
    },
    /// A stage completed.
    StageCompleted {
        /// Completed stage name.
        stage: String,
        /// Stage runtime.
        runtime_seconds: f64,
    },
    /// The run completed.
    RunCompleted {
        /// Terminal status label.
        status: String,
    },
    /// The run failed.
    RunFailed {
        /// Failure reason.
        reason: String,
    },
}

/// Context-defined work item table column.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressWorkItemColumn {
    /// Stable metric key.
    pub key: String,
    /// User-facing column label.
    pub label: String,
}

impl ProgressWorkItemColumn {
    /// Create a context-defined work item table column.
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }
}

/// Context-defined work item metric value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressWorkItemMetric {
    /// Stable metric key.
    pub key: String,
    /// User-facing metric value.
    pub value: String,
}

impl ProgressWorkItemMetric {
    /// Create a context-defined work item metric value.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Latest stage status shown in the terminal progress ledger.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProgressStageStatus {
    /// Stage has not started.
    #[default]
    Pending,
    /// Stage is active.
    Running,
    /// Stage completed successfully.
    Completed,
    /// Stage failed.
    Failed,
}

/// Latest semantic stage state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProgressStageSnapshot {
    /// User-facing stage name.
    pub name: String,
    /// One-based stage number.
    pub stage_number: usize,
    /// Stage status.
    pub status: ProgressStageStatus,
    /// Completed runtime label.
    pub elapsed: Option<String>,
}

/// Latest semantic work-item state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProgressWorkItemSnapshot {
    /// User-facing work item label.
    pub label: String,
    /// Progress label.
    pub progress: String,
    /// Elapsed runtime label.
    pub elapsed: String,
    /// Status label.
    pub status: String,
    /// Context-defined metric values.
    pub metrics: BTreeMap<String, String>,
}

/// Latest semantic progress state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProgressSnapshot {
    /// Stable run identifier.
    pub run_id: Option<String>,
    /// User-readable run scope.
    pub scope: Option<String>,
    /// Count of input groups or files.
    pub input_count: Option<usize>,
    /// Output path when known.
    pub output_path: Option<String>,
    /// Ordered stage states.
    pub stages: Vec<ProgressStageSnapshot>,
    /// Active stage name.
    pub active_stage: Option<String>,
    /// One-based active stage number.
    pub stage_number: Option<usize>,
    /// Total stage count.
    pub stage_count: Option<usize>,
    /// Current phase or subphase.
    pub phase: Option<String>,
    /// Current progress measure.
    pub measure: Option<ProgressMeasure>,
    /// Latest meaningful signal.
    pub latest_signal: Option<String>,
    /// Active work items.
    pub work_items: Vec<ProgressWorkItemSnapshot>,
    /// Context-defined work item columns.
    pub work_item_columns: Vec<ProgressWorkItemColumn>,
    /// Most recently completed stage.
    pub completed_stage: Option<String>,
    /// Terminal run status.
    pub terminal_status: Option<String>,
    /// Terminal failure reason.
    pub failure_reason: Option<String>,
}

/// Progress adapter over semantic progress events.
pub struct ProgressAdapter {
    snapshot: ProgressSnapshot,
    run_started_at: Option<Instant>,
    active_stage_started_at: Option<Instant>,
    active_work_started_at_by_label: BTreeMap<String, Instant>,
    progress_bar: ProgressBar,
    render_state: Arc<Mutex<ProgressRenderState>>,
    message_refresh_stop: Option<mpsc::Sender<()>>,
    message_refresh_thread: Option<JoinHandle<()>>,
    message_refresh_enabled: bool,
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
            message_refresh_stop: None,
            message_refresh_thread: None,
            message_refresh_enabled: true,
            steady_tick_enabled: false,
            dirty: false,
            color_enabled,
        }
    }

    /// Create a hidden adapter for tests and machine output modes.
    pub fn hidden() -> Self {
        let mut adapter = Self::new(ProgressDrawTarget::hidden());
        adapter.message_refresh_enabled = false;

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
                        status: ProgressStageStatus::Pending,
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
                self.snapshot.phase = None;
                self.snapshot.measure = None;
                self.snapshot.latest_signal = None;
                self.active_stage_started_at = Some(Instant::now());
                self.snapshot.work_items.clear();
                self.snapshot.work_item_columns.clear();
                self.active_work_started_at_by_label.clear();

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
                self.snapshot.phase = None;
                self.snapshot.measure = None;
                self.snapshot.latest_signal = None;
                self.snapshot.work_items.clear();
                self.snapshot.work_item_columns.clear();
                self.active_work_started_at_by_label.clear();
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
        self.stop_message_refresh();
        self.progress_bar.finish_and_clear();
    }

    /// Finish the live progress section while leaving the final rows on screen.
    pub fn finish_and_keep(&mut self) -> io::Result<()> {
        self.stop_message_refresh();
        self.progress_bar
            .finish_with_message(self.progress_message());

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
        if !self.message_refresh_enabled || self.message_refresh_thread.is_some() {
            return;
        }

        let (stop_sender, stop_receiver) = mpsc::channel();
        let progress_bar = self.progress_bar.clone();
        let render_state = Arc::clone(&self.render_state);

        self.message_refresh_stop = Some(stop_sender);

        self.message_refresh_thread = Some(thread::spawn(move || {
            loop {
                match stop_receiver.recv_timeout(MESSAGE_REFRESH_INTERVAL) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        let Ok(render_state) = render_state.lock() else {
                            break;
                        };
                        let message = render_state.progress_message();

                        drop(render_state);

                        progress_bar.set_message(message);
                        progress_bar.tick();
                    }
                }
            }
        }));
    }

    fn stop_message_refresh(&mut self) {
        if let Some(stop_sender) = self.message_refresh_stop.take() {
            let _ = stop_sender.send(());
        }

        if let Some(message_refresh_thread) = self.message_refresh_thread.take() {
            let _ = message_refresh_thread.join();
        }
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

impl Drop for ProgressAdapter {
    fn drop(&mut self) {
        self.stop_message_refresh();
    }
}

impl ProgressRenderState {
    fn progress_message(&self) -> String {
        let mut lines = Vec::new();
        let scope = self.snapshot.scope.as_deref().unwrap_or("Working");
        let total_elapsed = self.total_elapsed_label();

        if let Some(reason) = &self.snapshot.failure_reason {
            lines.push(self.workflow_status_line(scope, "failed", total_elapsed.as_deref()));
            lines.push(format!("   {}", self.labeled_value("Reason:", reason)));
        } else if let Some(status) = &self.snapshot.terminal_status {
            lines.push(self.workflow_status_line(scope, status, total_elapsed.as_deref()));
        } else if let Some(stage) = &self.snapshot.active_stage {
            if let (Some(stage_number), Some(stage_count)) =
                (self.snapshot.stage_number, self.snapshot.stage_count)
            {
                let elapsed = total_elapsed.unwrap_or_else(|| "00:00:00".to_string());

                lines.push(format!(
                    "{} {stage_number}/{stage_count}: {}   {} {}",
                    self.styled_active(scope),
                    self.styled_value(stage),
                    self.styled_label("Total elapsed:"),
                    self.styled_value(&elapsed)
                ));
            } else {
                let elapsed = total_elapsed.unwrap_or_else(|| "00:00:00".to_string());

                lines.push(format!(
                    "{}: {}   {} {}",
                    self.styled_active(scope),
                    self.styled_value(stage),
                    self.styled_label("Total elapsed:"),
                    self.styled_value(&elapsed)
                ));
            }
        } else {
            lines.push(self.workflow_status_line(scope, "running", total_elapsed.as_deref()));

            if let Some(run_id) = &self.snapshot.run_id {
                lines.push(format!("   {}", self.labeled_value("Run:", run_id)));
            }
            if let Some(input_count) = self.snapshot.input_count {
                lines.push(format!(
                    "   {}",
                    self.labeled_value("Inputs:", &input_count.to_string())
                ));
            }
            if let Some(output_path) = &self.snapshot.output_path {
                lines.push(format!("   {}", self.labeled_value("Output:", output_path)));
            }
        }

        if !self.snapshot.stages.is_empty() {
            lines.push(String::new());
            lines.extend(self.stage_lines());
        }

        if let Some(phase) = &self.snapshot.phase {
            lines.push(String::new());
            lines.push(format!("   {}", self.labeled_value("Phase:", phase)));
        }

        match (self.snapshot.measure, &self.snapshot.latest_signal) {
            (
                Some(ProgressMeasure::Bounded {
                    completed_units,
                    total_units,
                }),
                Some(signal),
            ) => {
                let progress = self.styled_progress_bar(completed_units, total_units);
                let signal = self.styled_value(signal);

                lines.push(format!(
                    "   {} {progress}, {signal}",
                    self.styled_label("Progress:")
                ));
            }
            (
                Some(ProgressMeasure::Bounded {
                    completed_units,
                    total_units,
                }),
                None,
            ) => lines.push(format!(
                "   {} {}",
                self.styled_label("Progress:"),
                self.styled_progress_bar(completed_units, total_units)
            )),
            (Some(ProgressMeasure::Unbounded), Some(signal)) => {
                lines.push(format!(
                    "   {} {}",
                    self.styled_label("Progress:"),
                    self.styled_value(signal)
                ));
            }
            (Some(ProgressMeasure::Unbounded), None) | (None, _) => {}
        }

        if !self.snapshot.work_items.is_empty() {
            lines.push(String::new());
            lines.extend(self.work_item_lines());
        }

        lines.join("\n")
    }

    fn stage_lines(&self) -> Vec<String> {
        let stage_count = self
            .snapshot
            .stage_count
            .unwrap_or(self.snapshot.stages.len());
        let stage_name_width = self
            .snapshot
            .stages
            .iter()
            .map(|stage| measure_text_width(stage.name.as_str()))
            .max()
            .unwrap_or_default();
        let status_width = "completed".len();

        self.snapshot
            .stages
            .iter()
            .map(|stage| {
                let elapsed = match stage.status {
                    ProgressStageStatus::Running => self.active_stage_elapsed_label(),
                    _ => stage.elapsed.clone(),
                };
                let status = stage_status_text(stage.status);
                let elapsed = elapsed
                    .map(|elapsed| format!("({elapsed})"))
                    .unwrap_or_default();
                let line = format!(
                    "{} {}/{}: {:<stage_name_width$}   {:<status_width$} {}",
                    stage_status_marker(stage.status),
                    stage.stage_number,
                    stage_count,
                    stage.name,
                    status,
                    elapsed
                );

                self.style_stage_line(stage.status, &line)
            })
            .collect()
    }

    fn active_stage_elapsed_label(&self) -> Option<String> {
        self.active_stage_started_at
            .map(|started_at| format_elapsed_duration(started_at.elapsed()))
    }

    fn total_elapsed_label(&self) -> Option<String> {
        self.run_started_at
            .map(|started_at| format_elapsed_duration(started_at.elapsed()))
    }

    fn work_item_lines(&self) -> Vec<String> {
        let columns = self.work_item_table_columns();
        let header = format_work_item_table_row(
            columns.as_slice(),
            columns
                .iter()
                .map(|column| column.label.as_str())
                .collect::<Vec<_>>()
                .as_slice(),
        );
        let mut lines = vec![self.styled(SemanticStyle::TableHeader, &header)];

        let mut work_items = self.snapshot.work_items.iter().collect::<Vec<_>>();
        work_items.sort_by_key(|work_item| work_item_status_sort_rank(&work_item.status));

        lines.extend(work_items.into_iter().map(|work_item| {
            let elapsed = if work_item_status_is_completed(&work_item.status) {
                work_item.elapsed.clone()
            } else {
                self.active_work_started_at_by_label
                    .get(work_item.label.as_str())
                    .map(|started_at| format_elapsed_duration(started_at.elapsed()))
                    .unwrap_or_else(|| work_item.elapsed.clone())
            };
            let values = columns
                .iter()
                .map(|column| match column.key.as_str() {
                    "label" => work_item.label.as_str(),
                    "progress" => work_item.progress.as_str(),
                    "elapsed" => elapsed.as_str(),
                    "status" => work_item.status.as_str(),
                    key => work_item
                        .metrics
                        .get(key)
                        .map(String::as_str)
                        .unwrap_or("-"),
                })
                .collect::<Vec<_>>();

            self.styled(
                SemanticStyle::Value,
                &format_work_item_table_row(columns.as_slice(), values.as_slice()),
            )
        }));

        lines
    }

    fn work_item_table_columns(&self) -> Vec<WorkItemTableColumn> {
        let mut columns = vec![
            WorkItemTableColumn::new("label", "Work item", WORK_ITEM_LABEL_COLUMN_WIDTH),
            WorkItemTableColumn::new("progress", "Progress", WORK_ITEM_PROGRESS_COLUMN_WIDTH),
            WorkItemTableColumn::new("elapsed", "Elapsed", WORK_ITEM_ELAPSED_COLUMN_WIDTH),
            WorkItemTableColumn::new("status", "Status", WORK_ITEM_STATUS_COLUMN_WIDTH),
        ];

        columns.extend(self.snapshot.work_item_columns.iter().map(|column| {
            WorkItemTableColumn::new(
                column.key.as_str(),
                column.label.as_str(),
                self.context_work_item_column_width(column),
            )
        }));

        columns
    }

    fn context_work_item_column_width(&self, column: &ProgressWorkItemColumn) -> usize {
        let label_width = measure_text_width(column.label.as_str());
        let value_width = self
            .snapshot
            .work_items
            .iter()
            .filter_map(|work_item| work_item.metrics.get(column.key.as_str()))
            .map(|value| measure_text_width(value.as_str()))
            .max()
            .unwrap_or_default();

        label_width.max(value_width).clamp(
            CONTEXT_WORK_ITEM_COLUMN_MIN_WIDTH,
            CONTEXT_WORK_ITEM_COLUMN_MAX_WIDTH,
        )
    }

    fn workflow_status_line(
        &self,
        scope: &str,
        status: &str,
        total_elapsed: Option<&str>,
    ) -> String {
        let elapsed = total_elapsed.unwrap_or("00:00:00");

        format!(
            "{}: {}   {} {}",
            self.styled_active(scope),
            self.styled_value(status),
            self.styled_label("Total elapsed:"),
            self.styled_value(elapsed)
        )
    }

    fn styled_progress_bar(&self, completed_units: usize, total_units: usize) -> String {
        let percent = progress_percent(completed_units, total_units);
        let bar = progress_bar_label(completed_units, total_units);

        format!(
            "{percent:>3}% {} {completed_units}/{total_units}",
            self.styled_bar(&bar)
        )
    }

    fn styled_bar(&self, text: &str) -> String {
        self.styled(SemanticStyle::ActiveStage, text)
    }

    fn styled_active(&self, text: &str) -> String {
        self.styled(SemanticStyle::ActiveStage, text)
    }

    fn style_stage_line(&self, status: ProgressStageStatus, text: &str) -> String {
        match status {
            ProgressStageStatus::Pending => self.styled(SemanticStyle::Metadata, text),
            ProgressStageStatus::Running => self.styled(SemanticStyle::ActiveStage, text),
            ProgressStageStatus::Completed => self.styled(SemanticStyle::CompletedMarker, text),
            ProgressStageStatus::Failed => self.styled(SemanticStyle::FailureMarker, text),
        }
    }

    fn labeled_value(&self, label: &str, value: &str) -> String {
        format!("{} {}", self.styled_label(label), self.styled_value(value))
    }

    fn styled_label(&self, text: &str) -> String {
        self.styled(SemanticStyle::Label, text)
    }

    fn styled_value(&self, text: &str) -> String {
        self.styled(SemanticStyle::Value, text)
    }

    fn styled(&self, style: SemanticStyle, text: &str) -> String {
        style_contract(style)
            .force_styling(self.color_enabled)
            .apply_to(text)
            .to_string()
    }
}

fn format_work_item_table_row(columns: &[WorkItemTableColumn], values: &[&str]) -> String {
    let mut row = String::from("   ");

    for (column_index, column) in columns.iter().enumerate() {
        if column_index > 0 {
            row.push(' ');
        }

        let value = values.get(column_index).copied().unwrap_or("-");
        let value = truncate_for_table(value, column.width);
        let formatted_value = format!("{value:<width$}", width = column.width);

        row.push_str(formatted_value.as_str());
    }

    row
}
