use crate::renderer::TerminalRenderer;

/// Terminal policy for rendering start and completion summaries.
#[derive(Clone, Copy, Debug)]
pub struct TerminalSummaryPolicy {
    /// Renderer used for stdout summaries.
    pub renderer: TerminalRenderer,
    /// Whether to emit the start summary.
    pub emit_start_summary: bool,
    /// Whether to emit the completion summary.
    pub emit_completion_summary: bool,
}

impl TerminalSummaryPolicy {
    /// Emit both start and completion summaries through the provided renderer.
    pub const fn start_and_completion(renderer: TerminalRenderer) -> Self {
        Self {
            renderer,
            emit_start_summary: true,
            emit_completion_summary: true,
        }
    }

    /// Emit only the completion summary through the provided renderer.
    pub const fn completion_only(renderer: TerminalRenderer) -> Self {
        Self {
            renderer,
            emit_start_summary: false,
            emit_completion_summary: true,
        }
    }
}
