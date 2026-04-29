//! Validation workflow progress contracts.

use janiform_terminal_ui::{ProgressWorkItemColumn, ProgressWorkItemMetric};

/// Progress phase emitted by the stylized-facts validation workflow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StylizedFactsValidationProgressPhase {
    /// Load and normalize empirical data.
    LoadEmpiricalData,
    /// Derive returns for requested intervals.
    DeriveIntervalReturns,
    /// Simulate seeded model paths.
    SimulateSeededPath,
    /// Compute stylized-fact metrics.
    ComputeMetrics,
    /// Compare metrics and assemble the report.
    CompareMetrics,
    /// Write output artifacts.
    WriteArtifacts,
}

/// Progress event emitted by the stylized-facts validation workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StylizedFactsValidationProgressEvent {
    /// Current workflow phase.
    pub phase: StylizedFactsValidationProgressPhase,
    /// Human-readable phase label.
    pub label: String,
    /// One-based phase step.
    pub step: usize,
    /// Total phase count.
    pub total_steps: usize,
    /// Completed work units within the current phase.
    pub completed_units: Option<usize>,
    /// Total work units within the current phase.
    pub total_units: Option<usize>,
    /// Active work item label.
    pub work_item_label: Option<String>,
    /// Active work item completion percentage.
    pub work_item_progress_percent: Option<usize>,
    /// Work items expected during the current phase.
    pub planned_work_item_labels: Vec<String>,
    /// Work item table columns.
    pub work_item_columns: Vec<ProgressWorkItemColumn>,
    /// Work item table metrics for the active work item.
    pub work_item_metrics: Vec<ProgressWorkItemMetric>,
    /// Latest meaningful progress signal.
    pub latest_signal: Option<String>,
}
