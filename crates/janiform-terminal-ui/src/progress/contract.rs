use std::collections::BTreeMap;
use std::num::TryFromIntError;

use thiserror::Error;

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
        /// Number of leading stages that completed before this progress view started.
        completed_stage_count: usize,
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
