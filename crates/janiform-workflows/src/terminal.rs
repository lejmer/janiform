use std::io;

use janiform_terminal_ui::{
    CompletionSummary, ProgressAdapter, ProgressEvent, StartSummary, TerminalRenderer,
    TerminalSummaryPolicy,
};
use serde::Deserialize;

use crate::error::WorkflowError;

/// Terminal progress mode requested by an entrypoint boundary.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalProgressMode {
    /// Render human terminal progress.
    Human,
    /// Do not render terminal progress.
    #[default]
    None,
}

impl TerminalProgressMode {
    /// Parse a boundary string into a terminal progress mode.
    pub fn from_boundary_value(value: &str) -> Result<Self, String> {
        match value {
            "human" => Ok(Self::Human),
            "none" => Ok(Self::None),
            _ => Err(format!(
                "progress_mode must be either 'human' or 'none', received '{value}'"
            )),
        }
    }
}

/// Build the stderr progress adapter for a terminal progress mode.
pub fn stderr_progress_adapter(progress_mode: TerminalProgressMode) -> Option<ProgressAdapter> {
    match progress_mode {
        TerminalProgressMode::Human => Some(ProgressAdapter::stderr()),
        TerminalProgressMode::None => None,
    }
}

/// Build the stdout renderer used by workflow start and completion views.
pub fn stdout_terminal_renderer() -> TerminalRenderer {
    TerminalRenderer::new(TerminalRenderer::stdout_color_enabled(), None)
}

/// Build the stdout start and completion summary policy for a terminal progress mode.
pub fn terminal_summary_policy(
    progress_mode: TerminalProgressMode,
) -> Option<TerminalSummaryPolicy> {
    match progress_mode {
        TerminalProgressMode::Human => Some(TerminalSummaryPolicy::start_and_completion(
            stdout_terminal_renderer(),
        )),
        TerminalProgressMode::None => None,
    }
}

/// Mark terminal progress as completed and keep the final view visible.
pub fn complete_terminal_progress(progress_adapter: &mut ProgressAdapter) -> io::Result<()> {
    progress_adapter.observe(ProgressEvent::RunCompleted {
        status: "completed".to_string(),
    });
    progress_adapter.finish_and_keep()
}

pub(crate) fn emit_terminal_start_summary(
    terminal_output: &Option<TerminalSummaryPolicy>,
    summary: StartSummary,
) -> Result<(), WorkflowError> {
    emit_terminal_start_view(terminal_output, |terminal_output| {
        Ok(summary.render(&terminal_output.renderer))
    })
}

pub(crate) fn emit_terminal_completion_summary(
    terminal_output: &Option<TerminalSummaryPolicy>,
    summary: CompletionSummary,
) -> Result<(), WorkflowError> {
    emit_terminal_completion_view(terminal_output, |terminal_output| {
        Ok(summary.render(&terminal_output.renderer))
    })
}

pub(crate) fn emit_terminal_start_view(
    terminal_output: &Option<TerminalSummaryPolicy>,
    render: impl FnOnce(&TerminalSummaryPolicy) -> Result<String, WorkflowError>,
) -> Result<(), WorkflowError> {
    let Some(terminal_output) = terminal_output else {
        return Ok(());
    };
    if !terminal_output.emit_start_summary {
        return Ok(());
    }

    let rendered = render(terminal_output)?;

    terminal_output
        .renderer
        .write_stdout(&format!("\n{rendered}"))
        .map_err(|source| WorkflowError::OutputWrite { source })
}

pub(crate) fn emit_terminal_completion_view(
    terminal_output: &Option<TerminalSummaryPolicy>,
    render: impl FnOnce(&TerminalSummaryPolicy) -> Result<String, WorkflowError>,
) -> Result<(), WorkflowError> {
    let Some(terminal_output) = terminal_output else {
        return Ok(());
    };
    if !terminal_output.emit_completion_summary {
        return Ok(());
    }

    let rendered = render(terminal_output)?;

    terminal_output
        .renderer
        .write_stdout(&rendered)
        .map_err(|source| WorkflowError::OutputWrite { source })
}
