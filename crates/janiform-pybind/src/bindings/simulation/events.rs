use janiform::simulation::SimulationProgressEvent;
use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::PyAny;

use crate::SimulationExecutionError;
use crate::bindings::events::{
    callback_error_state, callback_failed, emit_serializable_callback_event, raise_callback_error,
};
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
    TRun: FnOnce(TInput) -> PyResult<TOutput> + Send + 'static,
    TRunWithObserver: FnOnce(TInput, &mut dyn FnMut(SimulationProgressEvent)) -> PyResult<TOutput>
        + Send
        + 'static,
{
    let callback_error_state = callback_error_state();
    let callback_error_state_for_observer = callback_error_state.clone();

    let event_callback = event_callback.map(|callback| callback.clone().unbind());

    let run_output = panic_to_pyresult(panic_context, SimulationExecutionError::new_err, || {
        py.detach(move || match event_callback {
            None => run_without_observer(run_input),
            Some(event_callback) => {
                let mut observer =
                    simulation_progress_observer(event_callback, callback_error_state_for_observer);

                run_with_observer(run_input, &mut observer)
            }
        })
    })?;

    raise_callback_error(
        &callback_error_state,
        "simulation progress callback failed",
        SimulationExecutionError::new_err,
    )?;

    Ok(run_output)
}

fn simulation_progress_observer(
    event_callback: Py<PyAny>,
    callback_error_state: crate::bindings::events::CallbackErrorState,
) -> impl FnMut(SimulationProgressEvent) {
    move |event: SimulationProgressEvent| {
        if callback_failed(&callback_error_state) {
            return;
        }

        emit_serializable_callback_event(
            &event_callback,
            &callback_error_state,
            &event,
            "simulation progress event",
        );
    }
}
