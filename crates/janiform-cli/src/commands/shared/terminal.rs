use janiform_terminal_ui::ProgressAdapter;
use janiform_terminal_ui::TerminalSummaryPolicy;

use crate::context::{CommandContext, OutputMode};

pub(crate) fn terminal_output_for_context(
    context: &CommandContext,
) -> Option<TerminalSummaryPolicy> {
    match context.output_mode() {
        OutputMode::Human => Some(TerminalSummaryPolicy::start_and_completion(
            context.stdout_renderer(),
        )),
        OutputMode::Quiet => Some(TerminalSummaryPolicy::completion_only(
            context.stdout_renderer(),
        )),
        OutputMode::Json | OutputMode::Silent => None,
    }
}

pub(crate) fn terminal_progress_adapter_for_context(
    context: &CommandContext,
) -> Option<ProgressAdapter> {
    match context.output_mode() {
        OutputMode::Human | OutputMode::Quiet => Some(ProgressAdapter::stderr()),
        OutputMode::Json | OutputMode::Silent => None,
    }
}
