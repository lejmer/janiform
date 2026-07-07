use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use janiform::simulation::{SimulationProgressEvent, SimulationProgressPhase};
use janiform_foundations::paths::format_path;
use janiform_terminal_ui::{ProgressAdapter, ProgressEvent, TerminalSummaryPolicy};

use crate::error::WorkflowError;
use crate::progress::{VolatileProgressGate, bounded_progress_measure};
use crate::terminal::{
    complete_terminal_progress, emit_terminal_completion_view, emit_terminal_start_view,
};

use super::intervals::simulation_interval_progress_label;
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
        output_path: output_path.map(format_path),
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
    finish_simulation_progress(progress_adapter, progress_error, "completed")
}

pub(super) fn cancel_simulation_progress(
    progress_adapter: Option<&mut ProgressAdapter>,
    progress_error: Option<WorkflowError>,
) -> Result<(), WorkflowError> {
    finish_simulation_progress(progress_adapter, progress_error, "cancelled")
}

fn finish_simulation_progress(
    progress_adapter: Option<&mut ProgressAdapter>,
    progress_error: Option<WorkflowError>,
    status: &str,
) -> Result<(), WorkflowError> {
    if let Some(progress_error) = progress_error {
        return Err(progress_error);
    }

    let Some(progress_adapter) = progress_adapter else {
        return Ok(());
    };

    complete_terminal_progress(progress_adapter, status)
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
        SimulationProgressPhase::SeededTickIntervals => "Seeded tick intervals",
        SimulationProgressPhase::CoarseIntervals => "Coarse intervals",
    }
}

fn simulation_progress_signal(event: &SimulationProgressEvent) -> String {
    let interval_progress = simulation_interval_progress_label(
        event.completed_units,
        event.total_units,
        event.interval_duration_nanos,
    );

    match event.generated_tick_count {
        Some(generated_tick_count) => format!(
            "{interval_progress}, {}",
            generated_tick_count_label(generated_tick_count)
        ),
        None => interval_progress,
    }
}

fn generated_tick_count_label(generated_tick_count: u64) -> String {
    if generated_tick_count == 1 {
        return "1 tick".to_string();
    }

    format!("{generated_tick_count} ticks")
}

#[cfg(test)]
mod tests {
    use janiform::simulation::{SimulationProgressEvent, SimulationProgressPhase};
    use janiform_foundations::time::{NANOS_PER_FIFTEEN_MINUTES, NANOS_PER_HOUR};

    use super::simulation_progress_signal;

    #[test]
    fn seeded_progress_signal_uses_interval_unit_and_generated_tick_count() {
        let event = SimulationProgressEvent::new(
            SimulationProgressPhase::SeededTickIntervals,
            168,
            168,
            NANOS_PER_HOUR,
            Some(654_321),
        );

        assert_eq!(
            simulation_progress_signal(&event),
            "168/168 1h intervals, 654321 ticks"
        );
    }

    #[test]
    fn coarse_progress_signal_uses_interval_width_before_interval_word() {
        let event = SimulationProgressEvent::new(
            SimulationProgressPhase::CoarseIntervals,
            3,
            4,
            NANOS_PER_FIFTEEN_MINUTES,
            None,
        );

        assert_eq!(simulation_progress_signal(&event), "3/4 15min intervals");
    }
}
