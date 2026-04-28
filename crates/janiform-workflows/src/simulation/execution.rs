use std::time::Instant;

use janiform::simulation::{
    CoarseSimulationPath, SeededTickSimulationRunResult, SimulationProgressEvent,
    SimulationProgressPhase, run_coarse_simulation, run_coarse_simulation_with_observer,
    run_seeded_tick_event_simulation, run_seeded_tick_event_simulation_with_observer,
};

use crate::error::WorkflowError;
use crate::progress::VolatileProgressGate;

use super::model_artifact::run_simulation_from_model_artifact;
use super::output::{DirectSimulationWorkflowOutput, ModelArtifactSimulationWorkflowOutput};
use super::progress::{
    complete_simulation_progress, emit_completion_summary, emit_start_summary,
    observe_gated_simulation_terminal_event, start_simulation_progress,
};
use super::request::{
    CoarseSimulationWorkflowRequest, DirectSimulationWorkflowRequest, ModelArtifactSimulationKind,
    ModelArtifactSimulationWorkflowRequest, SeededTickEventSimulationWorkflowRequest,
};
use super::workflow::SimulationWorkflowObserver;

/// Run a simulation-from-model-artifact workflow.
pub fn run_simulation_from_model_artifact_workflow(
    request: ModelArtifactSimulationWorkflowRequest,
    mut observer: SimulationWorkflowObserver<'_>,
) -> Result<ModelArtifactSimulationWorkflowOutput, WorkflowError> {
    let scope_label = model_artifact_scope_label_for_request(&request).to_string();

    // Intentional copy because the workflow request is consumed by execution,
    // while completion rendering still needs the display output path.
    let output_path = request.output_path.clone();

    emit_start_summary(
        &observer.terminal_output,
        model_artifact_run_id(&request),
        scope_label.as_str(),
        request.request_path.as_deref(),
        output_path.as_deref(),
    )?;

    start_simulation_progress(
        observer.progress_adapter.as_deref_mut(),
        model_artifact_run_id(&request),
        scope_label.as_str(),
        1,
        output_path.as_deref(),
    )?;

    let mut progress_error = None::<WorkflowError>;
    let mut progress_gate = VolatileProgressGate::default();

    let mut progress_observer = |event: SimulationProgressEvent| {
        observe_gated_simulation_terminal_event(
            &mut observer.progress_adapter,
            &mut progress_gate,
            &mut progress_error,
            event,
        );
    };

    let output = run_simulation_from_model_artifact(request, &mut progress_observer)?;

    drop(progress_observer);

    complete_simulation_progress(observer.progress_adapter, progress_error)?;

    emit_completion_summary(
        &observer.terminal_output,
        scope_label.as_str(),
        output_path.as_deref(),
        output.export_summary.as_ref(),
        output.execution_duration,
        output.completed.completion(),
    )?;

    Ok(output)
}

/// Run a direct simulation workflow.
pub fn run_direct_simulation_workflow(
    request: DirectSimulationWorkflowRequest,
    mut observer: SimulationWorkflowObserver<'_>,
) -> Result<DirectSimulationWorkflowOutput, WorkflowError> {
    emit_start_summary(
        &observer.terminal_output,
        request.run_id.as_str(),
        request.scope_label.as_str(),
        request.request_path.as_deref(),
        request.output_path.as_deref(),
    )?;

    start_simulation_progress(
        observer.progress_adapter.as_deref_mut(),
        request.run_id.as_str(),
        request.scope_label.as_str(),
        1,
        request.output_path.as_deref(),
    )?;

    let mut progress_error = None::<WorkflowError>;
    let mut progress_gate = VolatileProgressGate::default();

    let mut progress_observer = |event: SimulationProgressEvent| {
        observe_gated_simulation_terminal_event(
            &mut observer.progress_adapter,
            &mut progress_gate,
            &mut progress_error,
            event,
        );
    };

    let started_at = Instant::now();
    let completed = request.job.run(&mut progress_observer)?;

    let execution_duration = started_at.elapsed();

    drop(progress_observer);

    let export_summary = completed.export(request.output_path.as_deref())?;

    complete_simulation_progress(observer.progress_adapter, progress_error)?;

    emit_completion_summary(
        &observer.terminal_output,
        request.scope_label.as_str(),
        request.output_path.as_deref(),
        export_summary.as_ref(),
        execution_duration,
        completed.completion(),
    )?;

    Ok(DirectSimulationWorkflowOutput {
        completed,
        export_summary,
        execution_duration,
    })
}

/// Run a coarse simulation workflow.
pub fn run_coarse_simulation_workflow(
    request: CoarseSimulationWorkflowRequest,
    observer: SimulationWorkflowObserver<'_>,
) -> Result<CoarseSimulationPath, WorkflowError> {
    run_direct_runtime_simulation_workflow(
        request.run_request,
        observer,
        "Simulating coarse intervals",
        run_coarse_simulation,
        run_coarse_simulation_with_observer,
    )
}

/// Run a seeded tick-event simulation workflow.
pub fn run_seeded_tick_event_simulation_workflow(
    request: SeededTickEventSimulationWorkflowRequest,
    observer: SimulationWorkflowObserver<'_>,
) -> Result<SeededTickSimulationRunResult, WorkflowError> {
    run_direct_runtime_simulation_workflow(
        request.run_config,
        observer,
        "Simulating seeded ticks",
        run_seeded_tick_event_simulation,
        run_seeded_tick_event_simulation_with_observer,
    )
}

fn run_direct_runtime_simulation_workflow<TInput, TRunResult, TRunError, TRun, TRunObserved>(
    input: TInput,
    mut observer: SimulationWorkflowObserver<'_>,
    scope: &str,
    run: TRun,
    run_observed: TRunObserved,
) -> Result<TRunResult, WorkflowError>
where
    TRun: FnOnce(TInput) -> Result<TRunResult, TRunError>,
    TRunObserved:
        FnOnce(TInput, &mut dyn FnMut(SimulationProgressEvent)) -> Result<TRunResult, TRunError>,
    TRunError: ToString,
    TRunResult: DirectSimulationCompletionFields,
{
    emit_start_summary(&observer.terminal_output, "simulation", scope, None, None)?;

    start_simulation_progress(
        observer.progress_adapter.as_deref_mut(),
        "simulation",
        scope,
        1,
        None,
    )?;

    let mut progress_error = None::<WorkflowError>;
    let mut progress_gate = VolatileProgressGate::<SimulationProgressPhase>::default();

    let started_at = Instant::now();

    let run_result = if observer.progress_adapter.is_some() {
        let mut progress_observer = |event: SimulationProgressEvent| {
            observe_gated_simulation_terminal_event(
                &mut observer.progress_adapter,
                &mut progress_gate,
                &mut progress_error,
                event,
            );
        };

        run_observed(input, &mut progress_observer)
    } else {
        run(input)
    }
    .map_err(|source| WorkflowError::SimulationValidationFailed {
        field: "request",
        message: source.to_string(),
    })?;

    let execution_duration = started_at.elapsed();

    complete_simulation_progress(observer.progress_adapter, progress_error)?;

    emit_completion_summary(
        &observer.terminal_output,
        scope,
        None,
        None,
        execution_duration,
        direct_completion_for_result(&run_result),
    )?;

    Ok(run_result)
}

fn model_artifact_run_id(request: &ModelArtifactSimulationWorkflowRequest) -> &str {
    request.run_id.as_deref().unwrap_or("simulation")
}

fn model_artifact_scope_label_for_request(
    request: &ModelArtifactSimulationWorkflowRequest,
) -> &str {
    request
        .scope_label
        .as_deref()
        .unwrap_or_else(|| model_artifact_scope_label(request.simulation_kind))
}

fn model_artifact_scope_label(simulation_kind: ModelArtifactSimulationKind) -> &'static str {
    match simulation_kind {
        ModelArtifactSimulationKind::SeededTick => "seeded tick simulation",
        ModelArtifactSimulationKind::CoarseInterval => "coarse interval simulation",
    }
}

trait DirectSimulationCompletionFields {
    fn seed(&self) -> u64;

    fn simulated_interval_count(&self) -> usize;
}

impl DirectSimulationCompletionFields for CoarseSimulationPath {
    fn seed(&self) -> u64 {
        self.seed
    }

    fn simulated_interval_count(&self) -> usize {
        self.simulated_interval_count
    }
}

impl DirectSimulationCompletionFields for SeededTickSimulationRunResult {
    fn seed(&self) -> u64 {
        self.seed
    }

    fn simulated_interval_count(&self) -> usize {
        self.simulated_interval_count
    }
}

fn direct_completion_for_result(
    result: &impl DirectSimulationCompletionFields,
) -> super::DirectSimulationCompletion {
    super::DirectSimulationCompletion {
        seed: result.seed(),
        simulated_interval_count: result.simulated_interval_count(),
    }
}
