use janiform_event_log::RunEvent;
use janiform_workflows::{error::WorkflowError, write_machine_event_to_stderr};
use pyo3::prelude::{Py, PyResult};
use pyo3::types::PyAny;

use crate::CalibrationExecutionError;
use crate::bindings::events::{
    CallbackErrorState, emit_serializable_callback_event, raise_callback_error,
};

pub(super) fn fit_machine_event_sink(
    emit_machine_events: bool,
    event_callback: Option<Py<PyAny>>,
    callback_error_state: CallbackErrorState,
) -> Option<impl FnMut(&RunEvent) -> Result<(), WorkflowError>> {
    if !emit_machine_events && event_callback.is_none() {
        return None;
    }

    Some(move |event: &RunEvent| {
        if emit_machine_events {
            write_machine_event_to_stderr(event)?;
        }

        if let Some(event_callback) = &event_callback {
            emit_serializable_callback_event(
                event_callback,
                &callback_error_state,
                event,
                "fit progress event",
            );
        }

        Ok(())
    })
}

pub(super) fn raise_fit_progress_callback_error(
    callback_error_state: &CallbackErrorState,
) -> PyResult<()> {
    raise_callback_error(
        callback_error_state,
        "fit progress callback failed",
        CalibrationExecutionError::new_err,
    )
}
