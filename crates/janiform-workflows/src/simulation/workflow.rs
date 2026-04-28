use janiform_terminal_ui::{ProgressAdapter, TerminalSummaryPolicy};

/// Observer sinks for simulation workflow execution.
#[derive(Default)]
pub struct SimulationWorkflowObserver<'a> {
    /// Terminal progress adapter.
    pub progress_adapter: Option<&'a mut ProgressAdapter>,
    /// Terminal output for run start and completion summaries.
    pub terminal_output: Option<TerminalSummaryPolicy>,
}
