use std::sync::mpsc;

use janiform::simulation::SimulationProgressEvent;
use janiform_foundations::cancellation::CancellationToken;
use pyo3::prelude::{Bound, PyResult, Python};
use pyo3::types::PyAny;

use crate::SimulationExecutionError;
use crate::bindings::events::{
    callback_error_state, callback_failed, emit_serializable_callback_event, raise_callback_error,
};
use crate::native_interrupt::{NativeInterruptMode, run_interruptible_native_with_poll};
use crate::panic_boundary::panic_to_pyresult;

pub(super) fn run_simulation_with_optional_progress_events<
    TInput,
    TOutput,
    TRun,
    TRunWithObserver,
>(
    py: Python<'_>,
    panic_context: &'static str,
    run_input: TInput,
    event_callback: Option<&Bound<'_, PyAny>>,
    run_without_observer: TRun,
    run_with_observer: TRunWithObserver,
) -> PyResult<TOutput>
where
    TInput: Send + 'static,
    TOutput: Send + 'static,
    TRun: FnOnce(TInput, CancellationToken) -> PyResult<TOutput> + Send + 'static,
    TRunWithObserver: FnOnce(
            TInput,
            CancellationToken,
            &mut dyn FnMut(SimulationProgressEvent),
        ) -> PyResult<TOutput>
        + Send
        + 'static,
{
    let callback_error_state = callback_error_state();
    let callback_error_state_for_observer = callback_error_state.clone();

    let (event_sender, event_receiver) = mpsc::channel::<SimulationProgressEvent>();

    let event_callback = event_callback.map(|callback| callback.clone().unbind());
    let event_callback_enabled = event_callback.is_some();

    let run_output = panic_to_pyresult(panic_context, SimulationExecutionError::new_err, || {
        run_interruptible_native_with_poll(
            py,
            NativeInterruptMode::ImportedApi,
            move |cancellation_token| {
                if !event_callback_enabled {
                    return run_without_observer(run_input, cancellation_token);
                }

                let mut observer =
                    simulation_progress_observer(event_sender, callback_error_state_for_observer);

                run_with_observer(run_input, cancellation_token, &mut observer)
            },
            |_: Python<'_>| {
                if let Some(event_callback) = event_callback.as_ref() {
                    for event in event_receiver.try_iter() {
                        emit_serializable_callback_event(
                            event_callback,
                            &callback_error_state,
                            &event,
                            "simulation progress event",
                        );
                    }
                }

                Ok(())
            },
        )
    })?;

    raise_callback_error(
        &callback_error_state,
        "simulation progress callback failed",
        SimulationExecutionError::new_err,
    )?;

    Ok(run_output)
}

fn simulation_progress_observer(
    event_sender: mpsc::Sender<SimulationProgressEvent>,
    callback_error_state: crate::bindings::events::CallbackErrorState,
) -> impl FnMut(SimulationProgressEvent) {
    move |event: SimulationProgressEvent| {
        if callback_failed(&callback_error_state) {
            return;
        }

        let _ = event_sender.send(event);
    }
}
