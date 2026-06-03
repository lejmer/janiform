use std::time::Duration;
use std::time::Instant;

use janiform::pricing::{
    PricingProgressEvent, VanillaOptionGridBatchPricingExecutionInput,
    VanillaOptionGridBatchPricingRunResult, VanillaOptionSurfacePricingExecutionInput,
    VanillaOptionSurfacePricingRunResult,
    run_vanilla_option_grid_batch_pricing as run_vanilla_option_grid_batch_pricing_core,
    run_vanilla_option_grid_batch_pricing_with_observer as run_vanilla_option_grid_batch_pricing_core_with_observer,
    run_vanilla_option_surface_pricing as run_vanilla_option_surface_pricing_core,
    run_vanilla_option_surface_pricing_with_observer as run_vanilla_option_surface_pricing_core_with_observer,
};
use janiform_foundations::cancellation::CancellationToken;
use janiform_terminal_ui::{CompletionSummary, StartSummary};

use crate::error::WorkflowError;
use crate::progress::VolatileProgressGate;
use crate::terminal::{TerminalProgressMode, stderr_progress_adapter, terminal_summary_policy};

use super::FittedModelSingleOptionPricingOutput;
use super::execution::run_fitted_model_single_option_pricing_workflow;
use super::labels::{
    vanilla_option_grid_batch_pricing_progress_scope_label,
    vanilla_option_surface_pricing_progress_scope_label,
};
use super::output::{
    grid_batch_pricing_completion_summary, grid_batch_pricing_start_summary,
    surface_pricing_completion_summary, surface_pricing_start_summary,
};
use super::progress::{
    cancel_pricing_progress, complete_pricing_progress, emit_pricing_completion_summary,
    emit_pricing_start_summary, observe_pricing_terminal_event, start_pricing_progress,
};
use super::request::{
    FittedModelSingleOptionPricingRequest, FittedModelSingleOptionPricingWorkflowRequest,
};
use super::workflow::PricingWorkflowObserver;

/// Run fitted-model single-option pricing with shared terminal progress.
pub fn run_fitted_model_single_option_pricing_with_terminal_progress(
    request: FittedModelSingleOptionPricingRequest,
    cancellation_token: CancellationToken,
    progress_mode: TerminalProgressMode,
) -> Result<FittedModelSingleOptionPricingOutput, WorkflowError> {
    let mut progress_adapter = stderr_progress_adapter(progress_mode);

    run_fitted_model_single_option_pricing_workflow(
        FittedModelSingleOptionPricingWorkflowRequest::from_pricing_request(request),
        PricingWorkflowObserver {
            cancellation_token,
            progress_adapter: progress_adapter.as_mut(),
            terminal_output: terminal_summary_policy(progress_mode),
        },
    )
}

/// Run vanilla option grid-batch pricing with shared terminal progress.
pub fn run_vanilla_option_grid_batch_pricing_with_terminal_progress(
    execution_input: VanillaOptionGridBatchPricingExecutionInput,
    cancellation_token: CancellationToken,
    progress_mode: TerminalProgressMode,
) -> Result<VanillaOptionGridBatchPricingRunResult, WorkflowError> {
    run_pricing_with_terminal_progress(
        PricingTerminalProgressRunInput {
            execution_input,
            cancellation_token,
            progress_mode,
            scope: vanilla_option_grid_batch_pricing_progress_scope_label(),
            start_summary: grid_batch_pricing_start_summary,
            completion_summary: grid_batch_pricing_completion_summary,
        },
        run_vanilla_option_grid_batch_pricing_core,
        run_vanilla_option_grid_batch_pricing_core_with_observer,
    )
}

/// Run vanilla option surface pricing with shared terminal progress.
pub fn run_vanilla_option_surface_pricing_with_terminal_progress(
    execution_input: VanillaOptionSurfacePricingExecutionInput,
    cancellation_token: CancellationToken,
    progress_mode: TerminalProgressMode,
) -> Result<VanillaOptionSurfacePricingRunResult, WorkflowError> {
    run_pricing_with_terminal_progress(
        PricingTerminalProgressRunInput {
            execution_input,
            cancellation_token,
            progress_mode,
            scope: vanilla_option_surface_pricing_progress_scope_label(),
            start_summary: surface_pricing_start_summary,
            completion_summary: surface_pricing_completion_summary,
        },
        run_vanilla_option_surface_pricing_core,
        run_vanilla_option_surface_pricing_core_with_observer,
    )
}

struct PricingTerminalProgressRunInput<'a, TExecutionInput, TStartSummary, TCompletionSummary> {
    execution_input: TExecutionInput,
    cancellation_token: CancellationToken,
    progress_mode: TerminalProgressMode,
    scope: &'a str,
    start_summary: TStartSummary,
    completion_summary: TCompletionSummary,
}

fn run_pricing_with_terminal_progress<
    TExecutionInput,
    TRunResult,
    TRunError,
    TStartSummary,
    TCompletionSummary,
    TRunCore,
    TRunObserved,
>(
    input: PricingTerminalProgressRunInput<'_, TExecutionInput, TStartSummary, TCompletionSummary>,
    run_core: TRunCore,
    run_observed: TRunObserved,
) -> Result<TRunResult, WorkflowError>
where
    TStartSummary: FnOnce(&TExecutionInput) -> StartSummary,
    TCompletionSummary: FnOnce(&TRunResult, Duration) -> CompletionSummary,
    TRunCore: FnOnce(TExecutionInput) -> Result<TRunResult, TRunError>,
    TRunObserved: FnOnce(
        TExecutionInput,
        Option<&mut dyn FnMut(PricingProgressEvent)>,
    ) -> Result<TRunResult, TRunError>,
    TRunError: ToString,
{
    let PricingTerminalProgressRunInput {
        execution_input,
        cancellation_token,
        progress_mode,
        scope,
        start_summary,
        completion_summary,
    } = input;

    cancellation_token.check_cancelled()?;

    let mut progress_adapter = stderr_progress_adapter(progress_mode);

    let terminal_output = terminal_summary_policy(progress_mode);

    emit_pricing_start_summary(&terminal_output, start_summary(&execution_input))?;
    start_pricing_progress(progress_adapter.as_mut(), "pricing", scope, 1, None)?;

    let mut progress_error = None::<WorkflowError>;
    let mut progress_gate = VolatileProgressGate::default();

    let started_at = Instant::now();

    let run_result = if let Some(progress_adapter) = progress_adapter.as_mut() {
        let mut observer = |event: PricingProgressEvent| {
            if progress_error.is_some() {
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

            if let Err(source) = observe_pricing_terminal_event(progress_adapter, &event) {
                progress_error = Some(source);
            }
        };

        run_observed(execution_input, Some(&mut observer))
    } else {
        run_core(execution_input)
    }
    .map_err(|source| WorkflowError::PricingExecutionFailed {
        message: source.to_string(),
    })?;

    let execution_duration = started_at.elapsed();

    if let Err(source) = cancellation_token.check_cancelled() {
        cancel_pricing_progress(progress_adapter.as_mut())?;
        return Err(WorkflowError::Cancelled { source });
    }

    if let Some(progress_error) = progress_error {
        return Err(progress_error);
    }

    complete_pricing_progress(progress_adapter.as_mut())?;

    emit_pricing_completion_summary(
        &terminal_output,
        completion_summary(&run_result, execution_duration),
    )?;

    Ok(run_result)
}
