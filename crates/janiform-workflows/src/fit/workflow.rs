use janiform_event_log::RunEvent;
use janiform_terminal_ui::{ProgressAdapter, TerminalSummaryPolicy};

use crate::error::WorkflowError;

/// Optional observers for shared fit workflow execution.
#[derive(Default)]
pub struct FitWorkflowObserver<'a> {
    /// Terminal progress adapter.
    pub progress_adapter: Option<&'a mut ProgressAdapter>,
    /// Terminal output for run start and completion summaries.
    pub terminal_output: Option<TerminalSummaryPolicy>,
    /// Sink for machine-readable run events.
    pub machine_event_sink: Option<&'a mut dyn FnMut(&RunEvent) -> Result<(), WorkflowError>>,
}
