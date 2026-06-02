use janiform::pricing::{PricingProgressEvent, pricing_progress_phase_label};
use janiform_terminal_ui::{
    CompletionSummary, ProgressAdapter, ProgressEvent, StartSummary, TerminalSummaryPolicy,
};

use crate::error::WorkflowError;
use crate::progress::bounded_progress_measure;
use crate::terminal::{
    complete_terminal_progress, emit_terminal_completion_summary, emit_terminal_start_summary,
};

pub(super) fn start_pricing_progress(
    progress_adapter: Option<&mut ProgressAdapter>,
    run_id: &str,
    scope: &str,
    input_count: usize,
    output_path: Option<&std::path::Path>,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::RunStarted {
        run_id: run_id.to_string(),
        scope: scope.to_string(),
        input_count,
        output_path: output_path.map(janiform_terminal_ui::format_path),
        stage_sequence: Vec::new(),
        completed_stage_count: 0,
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

pub(super) fn observe_pricing_terminal_event(
    progress_adapter: &mut ProgressAdapter,
    event: &PricingProgressEvent,
) -> Result<(), WorkflowError> {
    let measure = bounded_progress_measure(event.completed_units, event.total_units)?;

    progress_adapter.observe(ProgressEvent::RunProgress {
        phase: pricing_progress_phase_label(event.phase).to_string(),
        measure,
        latest_signal: Some(pricing_progress_signal(event)),
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

pub(super) fn emit_pricing_start_summary(
    terminal_output: &Option<TerminalSummaryPolicy>,
    summary: StartSummary,
) -> Result<(), WorkflowError> {
    emit_terminal_start_summary(terminal_output, summary)
}

pub(super) fn emit_pricing_completion_summary(
    terminal_output: &Option<TerminalSummaryPolicy>,
    summary: CompletionSummary,
) -> Result<(), WorkflowError> {
    emit_terminal_completion_summary(terminal_output, summary)
}

pub(super) fn complete_pricing_progress(
    progress_adapter: Option<&mut ProgressAdapter>,
) -> Result<(), WorkflowError> {
    finish_pricing_progress(progress_adapter, "completed")
}

pub(super) fn cancel_pricing_progress(
    progress_adapter: Option<&mut ProgressAdapter>,
) -> Result<(), WorkflowError> {
    finish_pricing_progress(progress_adapter, "cancelled")
}

fn finish_pricing_progress(
    progress_adapter: Option<&mut ProgressAdapter>,
    status: &str,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter else {
        return Ok(());
    };

    complete_terminal_progress(progress_adapter, status)
}

fn pricing_progress_signal(event: &PricingProgressEvent) -> String {
    let mut parts = Vec::new();

    if let Some(group_index) = event.current_group_index {
        parts.push(format!("group {}", one_based(group_index)));
    }
    if let (Some(maturity_index), Some(total_maturities)) =
        (event.current_maturity_index, event.total_maturities)
    {
        parts.push(format!(
            "maturity {}/{}",
            one_based(maturity_index),
            total_maturities
        ));
    }
    if let (Some(interval_index), Some(total_intervals)) =
        (event.current_interval_index, event.total_intervals)
    {
        parts.push(format!(
            "interval {}/{}",
            one_based(interval_index),
            total_intervals
        ));
    }
    if let (Some(strike_index), Some(total_strikes)) =
        (event.current_strike_index, event.total_strikes)
    {
        parts.push(format!(
            "strike {}/{}",
            one_based(strike_index),
            total_strikes
        ));
    }

    if parts.is_empty() {
        pricing_progress_phase_label(event.phase).to_string()
    } else {
        parts.join(" | ")
    }
}

fn one_based(index: usize) -> usize {
    index + 1
}
