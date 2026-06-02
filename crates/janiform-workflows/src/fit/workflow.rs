use janiform_event_log::RunEvent;
use janiform_foundations::cancellation::CancellationToken;
use janiform_terminal_ui::{ProgressAdapter, TerminalSummaryPolicy};

use crate::error::WorkflowError;

/// Mutable sink for machine-readable fit run events.
pub type FitRunEventSink<'a> = &'a mut dyn FnMut(&RunEvent) -> Result<(), WorkflowError>;

/// Optional observers for shared fit workflow execution.
#[derive(Default)]
pub struct FitWorkflowObserver<'a> {
    /// Cooperative cancellation token for long-running workflow work.
    pub cancellation_token: CancellationToken,
    /// Terminal progress adapter.
    pub progress_adapter: Option<&'a mut ProgressAdapter>,
    /// Terminal output for run start and completion summaries.
    pub terminal_output: Option<TerminalSummaryPolicy>,
    /// Sink for machine-readable run events.
    pub machine_event_sink: Option<FitRunEventSink<'a>>,
}
