use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use janiform::simulation::{SimulationProgressEvent, SimulationProgressPhase};
use janiform_terminal_ui::{ProgressAdapter, ProgressEvent, TerminalSummaryPolicy};

use crate::error::WorkflowError;
use crate::progress::{VolatileProgressGate, bounded_progress_measure};
use crate::terminal::{
    complete_terminal_progress, emit_terminal_completion_view, emit_terminal_start_view,
};

use super::output::{simulation_completion_summary, simulation_start_summary};

pub(super) fn start_simulation_progress(
    progress_adapter: Option<&mut ProgressAdapter>,
    run_id: &str,
    scope: &str,
    input_count: usize,
    output_path: Option<&Path>,
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

pub(super) fn observe_gated_simulation_terminal_event(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    progress_gate: &mut VolatileProgressGate<SimulationProgressPhase>,
    progress_error: &mut Option<WorkflowError>,
    event: SimulationProgressEvent,
) {
    if progress_error.is_some() {
        return;
    }
    if progress_adapter.is_none() {
        return;
    }
    if !progress_gate.should_emit(
        Instant::now(),
        event.phase,
        event.completed_units,
        event.total_units,
    ) {
        return;
    }

    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return;
    };

    if let Err(source) = observe_simulation_terminal_event(progress_adapter, &event) {
        *progress_error = Some(source);
    }
}

pub(super) fn observe_simulation_terminal_event(
    progress_adapter: &mut ProgressAdapter,
    event: &SimulationProgressEvent,
) -> Result<(), WorkflowError> {
    let measure = bounded_progress_measure(event.completed_units, event.total_units)?;

    progress_adapter.observe(ProgressEvent::RunProgress {
        phase: simulation_progress_phase_label(event.phase).to_string(),
        measure,
        latest_signal: Some(simulation_progress_signal(event)),
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

pub(super) fn complete_simulation_progress(
    progress_adapter: Option<&mut ProgressAdapter>,
    progress_error: Option<WorkflowError>,
) -> Result<(), WorkflowError> {
    if let Some(progress_error) = progress_error {
        return Err(progress_error);
    }

    let Some(progress_adapter) = progress_adapter else {
        return Ok(());
    };

    complete_terminal_progress(progress_adapter, "completed");

    Ok(())
}

pub(super) fn emit_start_summary(
    terminal_output: &Option<TerminalSummaryPolicy>,
    run_id: &str,
    scope: &str,
    request_path: Option<&Path>,
    output_path: Option<&Path>,
) -> Result<(), WorkflowError> {
    emit_terminal_start_view(terminal_output, |terminal_output| {
        Ok(
            simulation_start_summary(run_id, scope, request_path, output_path)
                .render(&terminal_output.renderer),
        )
    })
}

pub(super) fn emit_completion_summary(
    terminal_output: &Option<TerminalSummaryPolicy>,
    scope: &str,
    output_path: Option<&Path>,
    export_summary: Option<&super::SimulationExportSummary>,
    execution_duration: Duration,
    completion: super::DirectSimulationCompletion,
) -> Result<(), WorkflowError> {
    emit_terminal_completion_view(terminal_output, |terminal_output| {
        Ok(simulation_completion_summary(
            scope,
            completion,
            output_path,
            export_summary,
            execution_duration,
        )
        .render(&terminal_output.renderer))
    })
}

fn simulation_progress_phase_label(phase: SimulationProgressPhase) -> &'static str {
    match phase {
        SimulationProgressPhase::SeededTickIntervals => "seeded tick intervals",
        SimulationProgressPhase::CoarseIntervals => "coarse intervals",
    }
}

fn simulation_progress_signal(event: &SimulationProgressEvent) -> String {
    format!("{}/{} intervals", event.completed_units, event.total_units)
}
