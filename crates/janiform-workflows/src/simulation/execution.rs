use std::path::Path;
use std::time::Instant;

use janiform::simulation::{
    CoarseSimulationPath, CoarseSimulationRunRequest, SeededTickSimulationRunConfig,
    SeededTickSimulationRunResult, SimulationProgressEvent, SimulationProgressPhase,
};
use janiform_foundations::cancellation::{CancellationError, CancellationToken};

use crate::error::WorkflowError;
use crate::progress::VolatileProgressGate;

use super::direct::{DirectSimulationCompleted, DirectSimulationJob};
use super::job::run_simulation_job;
use super::labels::{
    RuntimeSimulationKind, direct_simulation_scope_label, model_artifact_simulation_scope_label,
};
use super::model_artifact::run_simulation_from_model_artifact;
use super::output::SimulationWorkflowOutput;
use super::progress::{
    cancel_simulation_progress, complete_simulation_progress, emit_completion_summary,
    emit_start_summary, observe_gated_simulation_terminal_event, start_simulation_progress,
};
use super::request::{
    ModelArtifactSimulationWorkflowRequest, SimulationWorkflowMetadata, SimulationWorkflowRequest,
};
use super::workflow::SimulationWorkflowObserver;

struct SimulationWorkflowPresentation<'a> {
    run_id: &'a str,
    scope: &'a str,
    request_path: Option<&'a Path>,
    output_path: Option<&'a Path>,
}

struct SimulationWorkflowRunState {
    cancellation_token: CancellationToken,
    progress_error: Option<WorkflowError>,
    progress_gate: VolatileProgressGate<SimulationProgressPhase>,
}

impl SimulationWorkflowRunState {
    fn start(
        observer: &mut SimulationWorkflowObserver<'_>,
        presentation: SimulationWorkflowPresentation<'_>,
    ) -> Result<Self, WorkflowError> {
        let cancellation_token = observer.cancellation_token.shared_handle();
        cancellation_token.check_cancelled()?;

        emit_start_summary(
            &observer.terminal_output,
            presentation.run_id,
            presentation.scope,
            presentation.request_path,
            presentation.output_path,
        )?;

        start_simulation_progress(
            observer.progress_adapter.as_deref_mut(),
            presentation.run_id,
            presentation.scope,
            1,
            presentation.output_path,
        )?;

        Ok(Self {
            cancellation_token,
            progress_error: None,
            progress_gate: VolatileProgressGate::default(),
        })
    }

    fn observe_terminal_event(
        &mut self,
        observer: &mut SimulationWorkflowObserver<'_>,
        event: SimulationProgressEvent,
    ) {
        observe_gated_simulation_terminal_event(
            &mut observer.progress_adapter,
            &mut self.progress_gate,
            &mut self.progress_error,
            event,
        );
    }

    fn cancel_progress(
        self,
        observer: &mut SimulationWorkflowObserver<'_>,
    ) -> Result<(), WorkflowError> {
        cancel_simulation_progress(observer.progress_adapter.take(), self.progress_error)
    }

    fn cancel_with_error<T>(
        self,
        observer: &mut SimulationWorkflowObserver<'_>,
        source: CancellationError,
    ) -> Result<T, WorkflowError> {
        self.cancel_progress(observer)?;

        Err(WorkflowError::Cancelled { source })
    }

    fn complete_progress(
        self,
        observer: &mut SimulationWorkflowObserver<'_>,
    ) -> Result<(), WorkflowError> {
        complete_simulation_progress(observer.progress_adapter.take(), self.progress_error)
    }

    fn check_cancelled(&self) -> Result<(), CancellationError> {
        self.cancellation_token.check_cancelled()
    }

    fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.shared_handle()
    }
}

fn run_observed_simulation_workflow<TRequest>(
    request: TRequest,
    mut observer: SimulationWorkflowObserver<'_>,
    scope_label: &str,
    request_metadata: impl FnOnce(&TRequest) -> &SimulationWorkflowMetadata,
    run_request: impl FnOnce(
        TRequest,
        CancellationToken,
        &mut dyn FnMut(SimulationProgressEvent),
    ) -> Result<SimulationWorkflowOutput, WorkflowError>,
) -> Result<SimulationWorkflowOutput, WorkflowError> {
    let (output_path, mut run_state) = {
        let metadata = request_metadata(&request);

        // Intentional copy because workflow execution consumes the request,
        // while completion rendering still needs the display output path.
        let output_path = metadata.output_path.clone();

        let run_state = SimulationWorkflowRunState::start(
            &mut observer,
            simulation_presentation_from_metadata(metadata, scope_label),
        )?;

        (output_path, run_state)
    };

    let cancellation_token = run_state.cancellation_token();

    let output_result = {
        let mut progress_observer = |event: SimulationProgressEvent| {
            run_state.observe_terminal_event(&mut observer, event);
        };

        run_request(request, cancellation_token, &mut progress_observer)
    };

    let output = match output_result {
        Ok(output) => output,
        Err(WorkflowError::Cancelled { source }) => {
            return run_state.cancel_with_error(&mut observer, source);
        }
        Err(error) => return Err(error),
    };

    if let Err(source) = run_state.check_cancelled() {
        return run_state.cancel_with_error(&mut observer, source);
    }

    run_state.complete_progress(&mut observer)?;

    emit_completion_summary(
        &observer.terminal_output,
        scope_label,
        output_path.as_deref(),
        output.export_summary.as_ref(),
        output.execution_duration,
        output.completed.completion(),
    )?;

    Ok(output)
}

/// Run a simulation-from-model-artifact workflow.
pub fn run_simulation_from_model_artifact_workflow(
    request: ModelArtifactSimulationWorkflowRequest,
    observer: SimulationWorkflowObserver<'_>,
) -> Result<SimulationWorkflowOutput, WorkflowError> {
    let scope_label = model_artifact_simulation_scope_label(request.simulation_kind);

    run_observed_simulation_workflow(
        request,
        observer,
        scope_label,
        |request| &request.metadata,
        run_simulation_from_model_artifact,
    )
}

/// Run a direct simulation workflow.
pub fn run_simulation_workflow(
    request: SimulationWorkflowRequest,
    observer: SimulationWorkflowObserver<'_>,
) -> Result<SimulationWorkflowOutput, WorkflowError> {
    let scope_label = direct_simulation_scope_label(&request.job);

    run_simulation_workflow_with_scope(request, observer, scope_label)
}

fn run_simulation_workflow_with_scope(
    request: SimulationWorkflowRequest,
    observer: SimulationWorkflowObserver<'_>,
    scope_label: &str,
) -> Result<SimulationWorkflowOutput, WorkflowError> {
    run_observed_simulation_workflow(
        request,
        observer,
        scope_label,
        |request| &request.metadata,
        |request, cancellation_token, progress_observer| {
            run_simulation_job(
                request,
                cancellation_token,
                progress_observer,
                Instant::now(),
            )
        },
    )
}

/// Run a coarse simulation workflow.
pub fn run_coarse_simulation_workflow(
    request: CoarseSimulationRunRequest,
    observer: SimulationWorkflowObserver<'_>,
) -> Result<CoarseSimulationPath, WorkflowError> {
    let output = run_runtime_simulation_workflow(
        DirectSimulationJob::CoarseInterval {
            request: Box::new(request),
        },
        observer,
        RuntimeSimulationKind::CoarseInterval,
    )?;

    match output.completed {
        DirectSimulationCompleted::CoarseInterval { result } => Ok(*result),
        DirectSimulationCompleted::SeededTick { .. } => Err(unexpected_runtime_output_error(
            RuntimeSimulationKind::CoarseInterval,
        )),
    }
}

/// Run a seeded tick-event simulation workflow.
pub fn run_seeded_tick_event_simulation_workflow(
    run_config: SeededTickSimulationRunConfig,
    observer: SimulationWorkflowObserver<'_>,
) -> Result<SeededTickSimulationRunResult, WorkflowError> {
    let output = run_runtime_simulation_workflow(
        DirectSimulationJob::SeededTick {
            run_config: Box::new(run_config),
        },
        observer,
        RuntimeSimulationKind::SeededTick,
    )?;

    match output.completed {
        DirectSimulationCompleted::SeededTick { result } => Ok(*result),
        DirectSimulationCompleted::CoarseInterval { .. } => Err(unexpected_runtime_output_error(
            RuntimeSimulationKind::SeededTick,
        )),
    }
}

fn run_runtime_simulation_workflow(
    job: DirectSimulationJob,
    observer: SimulationWorkflowObserver<'_>,
    runtime_kind: RuntimeSimulationKind,
) -> Result<SimulationWorkflowOutput, WorkflowError> {
    run_simulation_workflow_with_scope(
        SimulationWorkflowRequest {
            metadata: SimulationWorkflowMetadata::default(),
            job,
            export_plan: None,
        },
        observer,
        runtime_kind.scope_label(),
    )
}

fn simulation_presentation_from_metadata<'a>(
    metadata: &'a SimulationWorkflowMetadata,
    scope: &'a str,
) -> SimulationWorkflowPresentation<'a> {
    SimulationWorkflowPresentation {
        run_id: metadata.run_id(),
        scope,
        request_path: metadata.request_path.as_deref(),
        output_path: metadata.output_path.as_deref(),
    }
}

fn unexpected_runtime_output_error(expected: RuntimeSimulationKind) -> WorkflowError {
    WorkflowError::SimulationValidationFailed {
        field: "request",
        message: format!(
            "workflow expected {} output",
            expected.expected_output_label()
        ),
    }
}
