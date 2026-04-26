use std::num::TryFromIntError;

use indicatif::ProgressBar;
use indicatif::ProgressDrawTarget;
use indicatif::ProgressStyle;
use thiserror::Error;

const BOUNDED_PROGRESS_TEMPLATE: &str = "{spinner} {msg}";
const UNBOUNDED_PROGRESS_TEMPLATE: &str = "{spinner} {msg}";

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
#[derive(Clone, Debug, Eq, PartialEq)]
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
        /// Latest meaningful signal.
        latest_signal: Option<String>,
    },
    /// A stage completed.
    StageCompleted {
        /// Completed stage name.
        stage: String,
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
    progress_bar: ProgressBar,
    dirty: bool,
}

impl ProgressAdapter {
    /// Create an adapter that writes live progress through the provided draw target.
    pub fn new(draw_target: ProgressDrawTarget) -> Self {
        let progress_bar = ProgressBar::with_draw_target(None, draw_target);
        if let Ok(style) = ProgressStyle::with_template(UNBOUNDED_PROGRESS_TEMPLATE) {
            progress_bar.set_style(style);
        }

        Self {
            snapshot: ProgressSnapshot::default(),
            progress_bar,
            dirty: false,
        }
    }

    /// Create a hidden adapter for tests and machine output modes.
    pub fn hidden() -> Self {
        Self::new(ProgressDrawTarget::hidden())
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
            } => {
                self.snapshot.run_id = Some(run_id);
                self.snapshot.scope = Some(scope);
                self.snapshot.input_count = Some(input_count);
                self.snapshot.output_path = output_path;
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
            }
            ProgressEvent::StageProgress {
                phase,
                measure,
                latest_signal,
            } => {
                self.snapshot.phase = Some(phase);
                self.snapshot.measure = Some(measure);
                self.snapshot.latest_signal = latest_signal;
            }
            ProgressEvent::StageCompleted { stage } => {
                self.snapshot.completed_stage = Some(stage);
                self.snapshot.phase = None;
                self.snapshot.measure = None;
                self.snapshot.latest_signal = None;

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

                self.progress_bar.reset();
                self.progress_bar.unset_length();
            }
        }

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

        let Some(measure) = self.snapshot.measure else {
            self.dirty = false;
            return Ok(());
        };

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
            }
            ProgressMeasure::Unbounded => {
                let progress_bar = self.progress_bar_for_unbounded();

                progress_bar.set_message(message);
                progress_bar.tick();
            }
        }

        self.dirty = false;
        Ok(())
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

    fn progress_message(&self) -> String {
        let mut lines = Vec::new();

        if let Some(stage) = &self.snapshot.active_stage {
            if let (Some(stage_number), Some(stage_count)) =
                (self.snapshot.stage_number, self.snapshot.stage_count)
            {
                lines.push(format!("{stage_number}/{stage_count}: {stage}"));
            } else {
                lines.push(stage.clone());
            }
        }
        if let Some(phase) = &self.snapshot.phase {
            lines.push(format!("   Phase: {phase}"));
        }

        match (self.snapshot.measure, &self.snapshot.latest_signal) {
            (
                Some(ProgressMeasure::Bounded {
                    completed_units,
                    total_units,
                }),
                Some(signal),
            ) => lines.push(format!(
                "   Progress: {completed_units}/{total_units}, {signal}"
            )),
            (
                Some(ProgressMeasure::Bounded {
                    completed_units,
                    total_units,
                }),
                None,
            ) => lines.push(format!("   Progress: {completed_units}/{total_units}")),
            (Some(ProgressMeasure::Unbounded), Some(signal)) => {
                lines.push(format!("   Progress: {signal}"));
            }
            (Some(ProgressMeasure::Unbounded), None) | (None, _) => {}
        }

        lines.join("\n")
    }
}
